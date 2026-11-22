//! SQLite repository via rusqlite.
//! Same schema (migrations/0001_init.sql), same invariants.

use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};

use std::collections::HashMap;

use super::crypto::{Crypto, ENVELOPE_PREFIX};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::{
    AiAction, CaseMode, ClipboardEntry, ContentType, Folder, Macro, OutChunk, Phrase,
    PhraseSummary, PhraseVersion, Segment, Settings, TriggerBinding, TriggerRequest, TriggerType,
};
use crate::domain::{evaluate_macro, now_iso, preview_text};

const MIGRATION_SQL: &str = include_str!("../../migrations/0001_init.sql");

fn db_err(e: rusqlite::Error) -> DomainError {
    DomainError::Validation(format!("database error: {e}"))
}

/// One-row existence probe: true if the query returns a row.
fn row_exists(conn: &Connection, sql: &str, id: &str) -> DomainResult<bool> {
    Ok(conn
        .query_row(sql, params![id], |_| Ok(true))
        .optional()
        .map_err(db_err)?
        .unwrap_or(false))
}

/// Best-effort read of the current clipboard text (for the Clipboard macro).
fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
        .filter(|s| !s.is_empty())
}

/// The first literal-text segment of a phrase (used for duplicate grouping).
fn first_text(p: &Phrase) -> Option<String> {
    for seg in &p.body {
        if let Segment::Text { text } = seg {
            return Some(text.clone());
        }
    }
    None
}

/// Canonical tag form: trimmed, lowercased, deduplicated, `#` stripped. Tags match
/// case-insensitively in search, so storage is canonical.
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in tags {
        let t = t.trim().trim_start_matches('#').to_lowercase();
        if !t.is_empty() && seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

fn content_type_str(c: ContentType) -> &'static str {
    match c {
        ContentType::Plain => "plain",
        ContentType::Markdown => "markdown",
        ContentType::Richtext => "richtext",
        ContentType::Html => "html",
        ContentType::Word => "word",
    }
}

fn case_mode_str(m: CaseMode) -> &'static str {
    match m {
        CaseMode::Adapt => "adapt",
        CaseMode::Always => "always",
        CaseMode::Never => "never",
    }
}

fn parse_case_mode(s: &str) -> CaseMode {
    match s {
        "always" => CaseMode::Always,
        "never" => CaseMode::Never,
        _ => CaseMode::Adapt,
    }
}

fn parse_content_type(s: &str) -> ContentType {
    match s {
        "markdown" => ContentType::Markdown,
        "richtext" => ContentType::Richtext,
        "html" => ContentType::Html,
        "word" => ContentType::Word,
        _ => ContentType::Plain,
    }
}

// --------------------------------------------------------------------------- //
// At-rest encryption helpers. Envelope is `enc1:<b64(nonce‖ct‖tag)>`
// stored inline in existing TEXT columns. `crypto == None` (tests, or a machine with no
// keychain) means the layer is inert: writes are plaintext, enveloped reads degrade.
// --------------------------------------------------------------------------- //

/// Body shown for an encrypted row whose key isn't available (keychain absent/wrong machine).
const LOCKED_PLACEHOLDER: &str = "[encrypted — keychain unavailable]";

/// Encrypt `plain` when crypto is available, else pass it through unchanged.
fn maybe_encrypt(crypto: Option<&Crypto>, plain: &str) -> String {
    crypto
        .and_then(|c| c.encrypt_str(plain))
        .unwrap_or_else(|| plain.to_string())
}

/// Decrypt an enveloped value. `Some(plaintext)` for plaintext passthrough or successful
/// decrypt; `None` when enveloped but undecryptable (locked) — callers should skip such rows.
fn maybe_decrypt(crypto: Option<&Crypto>, raw: &str) -> Option<String> {
    if raw.starts_with(ENVELOPE_PREFIX) {
        crypto.and_then(|c| c.decrypt_str(raw))
    } else {
        Some(raw.to_string())
    }
}

/// Decode a stored `body_json`. Plaintext passes through; a successfully-decrypted envelope
/// returns its JSON; an undecryptable envelope returns a single placeholder segment so the
/// reader never fails on an encrypted row (the phrase surfaces as locked, not corrupt).
fn decode_body_json(raw: &str, crypto: Option<&Crypto>) -> String {
    if let Some(plain) = maybe_decrypt(crypto, raw) {
        plain
    } else {
        serde_json::to_string(&[Segment::Text {
            text: LOCKED_PLACEHOLDER.to_string(),
        }])
        .unwrap_or_else(|_| "[{}]".to_string())
    }
}

pub struct Db {
    conn: Mutex<Connection>,
    /// Encryption layer (set once at desktop startup). `None` in tests / when the keychain is
    /// unavailable → all helpers above are inert and the layer behaves as plaintext.
    crypto: OnceLock<Arc<Crypto>>,
    /// Cached settings row (None = uncached). `load_settings` decrypts the API key
    /// through the OS keychain — a Secret Service round trip — and it was called per
    /// AI macro per expansion. Cleared by every settings writer.
    settings_cache: Mutex<Option<Arc<Settings>>>,
}

/// Snapshots retained per phrase (version history). Oldest pruned on every edit.
const MAX_PHRASE_VERSIONS: i64 = 50;

impl Db {
    pub fn open(path: &str) -> DomainResult<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        // WAL + synchronous=NORMAL: writes (record_usage, version snapshots) commit with
        // one fsync instead of a rollback journal's several, and readers stop blocking
        // writers — this db is on the keystroke path.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
            crypto: OnceLock::new(),
            settings_cache: Mutex::new(None),
        };
        db.run_migrations()?;
        Ok(db)
    }

    /// Attach the encryption layer. Called once at desktop startup (after open, before any
    /// read). Tests never call it, so they stay pure-plaintext.
    pub fn set_crypto(&self, crypto: Arc<Crypto>) {
        let _ = self.crypto.set(crypto);
    }

    /// True when encryption is active (a DEK is loaded).
    pub fn crypto_available(&self) -> bool {
        self.crypto.get().is_some_and(|c| c.is_available())
    }

    fn crypto(&self) -> Option<&Crypto> {
        self.crypto.get().map(|arc| arc.as_ref())
    }

    pub fn open_in_memory() -> DomainResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
            crypto: OnceLock::new(),
            settings_cache: Mutex::new(None),
        };
        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(MIGRATION_SQL).map_err(db_err)?;
        // Schema evolution: add AI settings columns if missing (ignore "duplicate column").
        for stmt in [
            "ALTER TABLE settings ADD COLUMN ai_enabled INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE settings ADD COLUMN ai_base_url TEXT NOT NULL DEFAULT 'https://api.groq.com/openai/v1'",
            "ALTER TABLE settings ADD COLUMN ai_api_key TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE settings ADD COLUMN ai_model TEXT NOT NULL DEFAULT 'llama-3.3-70b-versatile'",
            // Global brand-voice instructions sent as a system message with every AI call.
            "ALTER TABLE settings ADD COLUMN ai_instructions TEXT NOT NULL DEFAULT ''",
            // Sampling temperature + optional token cap for AI calls (0 = provider default).
            "ALTER TABLE settings ADD COLUMN ai_temperature REAL NOT NULL DEFAULT 1.0",
            "ALTER TABLE settings ADD COLUMN ai_max_tokens INTEGER NOT NULL DEFAULT 0",
            // Hotkey for the selection-based AI flow (rewrite selection in place).
            "ALTER TABLE settings ADD COLUMN ai_selection_hotkey TEXT NOT NULL DEFAULT 'Control+Alt+A'",
            // Window-title substrings where autotext expansion is disabled entirely.
            "ALTER TABLE settings ADD COLUMN autotext_exclude_apps TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE settings ADD COLUMN sync_passphrase TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE phrases ADD COLUMN app_scope TEXT",
            "ALTER TABLE settings ADD COLUMN spellcheck_enabled INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE settings ADD COLUMN case_adapt_enabled INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE settings ADD COLUMN spellcheck_ignore TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE settings ADD COLUMN spellcheck_mode TEXT NOT NULL DEFAULT 'normal'",
            "ALTER TABLE settings ADD COLUMN spellcheck_hotkey TEXT NOT NULL DEFAULT 'Control+Shift+Space'",
            "ALTER TABLE settings ADD COLUMN clipboard_max_length INTEGER NOT NULL DEFAULT 10000",
            "ALTER TABLE settings ADD COLUMN clipboard_max_entries INTEGER NOT NULL DEFAULT 100",
            "ALTER TABLE settings ADD COLUMN spellcheck_exclude_apps TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE settings ADD COLUMN spellcheck_delivery TEXT NOT NULL DEFAULT 'silent'",
            "ALTER TABLE settings ADD COLUMN start_minimized INTEGER NOT NULL DEFAULT 0",
            // File-based sync: target directory (empty = sync off). All other sync
            // state (manifest, last run) lives in the sync folder itself.
            "ALTER TABLE settings ADD COLUMN sync_dir TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE folders ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
            // At-rest encryption toggle. Default ON; inert when no keychain is present.
            "ALTER TABLE settings ADD COLUMN encrypt_at_rest INTEGER NOT NULL DEFAULT 1",
            // Opt-in sentence review (AI grammar pass offered after sentence end).
            // Default OFF — it sends typed text to the AI provider.
            "ALTER TABLE settings ADD COLUMN review_sentence INTEGER NOT NULL DEFAULT 0",
            // Learned spellcheck corrections (typo → correction), learned on accept and
            // unlearned on undo. A table (not a settings CSV) because it's an unbounded map.
            "CREATE TABLE IF NOT EXISTS spellcheck_learned (\
             typo TEXT PRIMARY KEY, correction TEXT NOT NULL)",
            // Usage stats (1.4): expansion counter + last-used stamp. Nullable/defaulted
            // so pre-existing DB files stay valid.
            "ALTER TABLE phrases ADD COLUMN expanded_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE phrases ADD COLUMN last_used_at TEXT",
            "ALTER TABLE phrases ADD COLUMN case_mode TEXT NOT NULL DEFAULT 'adapt'",
            // Free-form tags (JSON array of lowercase strings), searchable via #tag.
            "ALTER TABLE phrases ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
            // Persistent named counters for the counter macro.
            "CREATE TABLE IF NOT EXISTS counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL)",
            // User-editable AI action presets (1.6). Builtin rows seeded below —
            // INSERT OR IGNORE so user edits to builtins survive upgrades.
            "CREATE TABLE IF NOT EXISTS ai_actions (\
             id TEXT PRIMARY KEY, name TEXT NOT NULL, prompt_template TEXT NOT NULL, \
             sort_order INTEGER NOT NULL DEFAULT 0, builtin INTEGER NOT NULL DEFAULT 0)",
        ] {
            let _ = conn.execute(stmt, []);
        }
        for (i, (id, name, template)) in
            crate::domain::models::BUILTIN_AI_ACTIONS.iter().enumerate()
        {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO ai_actions (id, name, prompt_template, sort_order, builtin) \
                 VALUES (?1, ?2, ?3, ?4, 1)",
                params![id, name, template, i as i64],
            );
        }
        // Retire merged builtins: drop a row only while it still matches the
        // retired action's original text — user edits survive the merge.
        for (id, name, template, _target) in crate::domain::models::MERGED_AI_ACTIONS {
            let _ = conn.execute(
                "DELETE FROM ai_actions WHERE id = ?1 AND name = ?2 AND prompt_template = ?3",
                params![id, name, template],
            );
        }
        // Retemplate upgraded builtins in place — again only on the untouched text.
        for (id, old_template, new_template) in crate::domain::models::RETEMPLATED_AI_ACTIONS {
            let _ = conn.execute(
                "UPDATE ai_actions SET prompt_template = ?3 \
                 WHERE id = ?1 AND prompt_template = ?2",
                params![id, old_template, new_template],
            );
        }
        // 2026-11 menu revamp: converge untouched rows onto the curated 13-tool
        // menu — retitle, re-prompt, and re-sort (position in BUILTIN_AI_ACTIONS).
        // Matched on template verbatim, like the passes above; a user-edited
        // action keeps its text and its menu slot. Runs after retemplating so an
        // old DB walks old → pre-revamp → current in one launch.
        for (id, name, template, old_templates) in crate::domain::models::REVAMPED_AI_ACTIONS {
            let Some(order) = crate::domain::models::BUILTIN_AI_ACTIONS
                .iter()
                .position(|(bid, _, _)| *bid == id)
            else {
                continue;
            };
            for old in old_templates {
                let _ = conn.execute(
                    "UPDATE ai_actions SET name = ?2, prompt_template = ?3, sort_order = ?4 \
                     WHERE id = ?1 AND prompt_template = ?5",
                    params![id, name, template, order as i64, old],
                );
            }
        }
        // Retire builtins dropped by the revamp — untouched rows only, so a
        // user-edited action survives as a custom one.
        for (id, name, template) in crate::domain::models::RETIRED_AI_ACTIONS {
            let _ = conn.execute(
                "DELETE FROM ai_actions WHERE id = ?1 AND name = ?2 AND prompt_template = ?3",
                params![id, name, template],
            );
        }
        Ok(())
    }

    // ----------------------------------------------------------------------- //
    // Folders
    // ----------------------------------------------------------------------- //
    pub fn create_folder(&self, folder: &Folder) -> DomainResult<Folder> {
        let conn = self.conn.lock().unwrap();
        if let Some(parent) = &folder.parent_id {
            if !row_exists(&conn, "SELECT 1 FROM folders WHERE id = ?1", parent)? {
                return Err(DomainError::NotFound(format!(
                    "parent folder {parent} not found"
                )));
            }
            guard_no_cycle(&conn, &folder.id, parent)?;
        }
        conn.execute(
            "INSERT INTO folders (id, name, parent_id, created_at, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![folder.id, folder.name, folder.parent_id, folder.created_at, folder.enabled as i64],
        )
        .map_err(db_err)?;
        Ok(folder.clone())
    }

    /// True when any live phrase carries a non-empty `app_scope` — the poller's
    /// wake condition, checked without decrypting/parsing the whole library.
    pub fn has_scoped_phrase(&self) -> DomainResult<bool> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM phrases \
                 WHERE deleted_at IS NULL AND app_scope IS NOT NULL AND app_scope != '')",
                [],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        Ok(exists)
    }

    pub fn list_folders(&self) -> DomainResult<Vec<Folder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, parent_id, created_at, enabled FROM folders")
            .map_err(db_err)?;
        let rows = stmt.query_map([], row_to_folder).map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    /// Enable/disable a folder. Disabled folders' phrases are retained but don't trigger.
    /// Returns the new state, or None if the folder doesn't exist.
    pub fn set_folder_enabled(&self, id: &str, enabled: bool) -> DomainResult<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute(
                "UPDATE folders SET enabled = ?1 WHERE id = ?2",
                params![enabled as i64, id],
            )
            .map_err(db_err)?;
        Ok(if rows > 0 { Some(enabled) } else { None })
    }

    pub fn delete_folder(&self, id: &str) -> DomainResult<bool> {
        let conn = self.conn.lock().unwrap();
        if !row_exists(&conn, "SELECT 1 FROM folders WHERE id = ?1", id)? {
            return Ok(false);
        }
        // Refuse if active (non-deleted) phrases reference it.
        let has_active = row_exists(
            &conn,
            "SELECT 1 FROM phrases WHERE folder_id = ?1 AND deleted_at IS NULL LIMIT 1",
            id,
        )?;
        if has_active {
            return Err(DomainError::FolderNotEmpty(format!(
                "folder {id} still contains active phrases"
            )));
        }
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(true)
    }

    /// Merge `source` into `target`: every phrase (including soft-deleted ones, so
    /// an undelete never lands in a ghost folder) moves to `target`, then `source`
    /// is deleted. No phrase_versions snapshots — content is unchanged.
    /// Returns the number of phrases moved. `Ok(None)` = source folder not found;
    /// `Err(NotFound)` = target missing; `Err(FolderNotEmpty)` = source has child
    /// folders (nesting is flat today, but the check keeps this future-proof).
    pub fn merge_folders(&self, source_id: &str, target_id: &str) -> DomainResult<Option<u32>> {
        let mut conn = self.conn.lock().unwrap();
        if !row_exists(&conn, "SELECT 1 FROM folders WHERE id = ?1", source_id)? {
            return Ok(None);
        }
        if !row_exists(&conn, "SELECT 1 FROM folders WHERE id = ?1", target_id)? {
            return Err(DomainError::NotFound(format!(
                "target folder {target_id} not found"
            )));
        }
        if source_id == target_id {
            return Err(DomainError::Validation(
                "cannot merge a folder into itself".into(),
            ));
        }
        let has_children = row_exists(
            &conn,
            "SELECT 1 FROM folders WHERE parent_id = ?1 LIMIT 1",
            source_id,
        )?;
        if has_children {
            return Err(DomainError::FolderNotEmpty(format!(
                "folder {source_id} still contains child folders"
            )));
        }
        let tx = conn.transaction().map_err(db_err)?;
        // Bump updated_at on the moved phrases so other devices' live copies (still in
        // the old folder) lose the LWW comparison and the move propagates.
        let moved = tx
            .execute(
                "UPDATE phrases SET folder_id = ?1, updated_at = ?2 WHERE folder_id = ?3",
                params![target_id, now_iso(), source_id],
            )
            .map_err(db_err)? as u32;
        tx.execute("DELETE FROM folders WHERE id = ?1", params![source_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Some(moved))
    }

    // ----------------------------------------------------------------------- //
    // Phrases
    // ----------------------------------------------------------------------- //
    // Abbreviation uniqueness is intentionally NOT enforced — multiple phrases may share
    // an abbreviation; a multi-match selection menu resolves ambiguity.

    /// Encode a phrase into its stored column values: encrypted-if-sensitive
    /// body JSON, triggers JSON, canonical tags JSON.
    fn encode_phrase_row(&self, phrase: &Phrase) -> DomainResult<(String, String, String)> {
        let body_json = serde_json::to_string(&phrase.body)
            .map_err(|e| DomainError::Validation(format!("cannot serialize body: {e}")))?;
        let body_json = if phrase.sensitive {
            maybe_encrypt(self.crypto(), &body_json)
        } else {
            body_json
        };
        let triggers_json = serde_json::to_string(&phrase.triggers)
            .map_err(|e| DomainError::Validation(format!("cannot serialize triggers: {e}")))?;
        let tags_json = serde_json::to_string(&normalize_tags(&phrase.tags))
            .map_err(|e| DomainError::Validation(format!("cannot serialize tags: {e}")))?;
        Ok((body_json, triggers_json, tags_json))
    }

    pub fn create_phrase(&self, phrase: &Phrase) -> DomainResult<Phrase> {
        phrase.validate()?;
        {
            let conn = self.conn.lock().unwrap();
            if !row_exists(
                &conn,
                "SELECT 1 FROM folders WHERE id = ?1",
                &phrase.folder_id,
            )? {
                return Err(DomainError::NotFound(format!(
                    "folder {} not found",
                    phrase.folder_id
                )));
            }
        }
        let (body_json, triggers_json, tags_json) = self.encode_phrase_row(phrase)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO phrases \
             (id, folder_id, name, abbreviation, content_type, body_json, triggers_json, \
              sensitive, app_scope, expanded_count, last_used_at, case_mode, created_at, updated_at, deleted_at, tags) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13, NULL, ?14)",
            params![
                phrase.id,
                phrase.folder_id,
                phrase.name,
                phrase.abbreviation,
                content_type_str(phrase.content_type),
                body_json,
                triggers_json,
                phrase.sensitive as i64,
                phrase.app_scope,
                phrase.expanded_count as i64,
                case_mode_str(phrase.case_mode),
                phrase.created_at,
                phrase.updated_at,
                tags_json,
            ],
        )
        .map_err(db_err)?;
        Ok(phrase.clone())
    }

    pub fn get_phrase(&self, id: &str) -> DomainResult<Option<Phrase>> {
        let crypto = self.crypto();
        let conn = self.conn.lock().unwrap();
        get_phrase_inner(&conn, id, crypto)
    }

    // ----------------------------------------------------------------------- //
    // Sync (file-based library mirroring, last-writer-wins)
    // ----------------------------------------------------------------------- //
    /// Insert or replace a folder row from a sync source.
    pub fn upsert_synced_folder(&self, folder: &Folder) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO folders (id, name, parent_id, created_at, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
                name=excluded.name, parent_id=excluded.parent_id, enabled=excluded.enabled",
            params![
                folder.id,
                folder.name,
                folder.parent_id,
                folder.created_at,
                folder.enabled as i64
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// Insert or replace a phrase row verbatim from a sync source: every field —
    /// including `updated_at`, `deleted_at`, and usage stats — is taken from
    /// `phrase`. Unlike `update_phrase`, no timestamps are bumped (otherwise the
    /// two devices would each see a "newer" copy after every sync and ping-pong
    /// forever). The previous row is snapshotted first, so an overwrite stays
    /// recoverable via version history.
    pub fn upsert_synced_phrase(&self, phrase: &Phrase) -> DomainResult<()> {
        phrase.validate()?;
        let (body_json, triggers_json, tags_json) = self.encode_phrase_row(phrase)?;
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO phrases \
                 (id, folder_id, name, abbreviation, content_type, body_json, triggers_json, \
                  sensitive, app_scope, expanded_count, last_used_at, case_mode, created_at, \
                  updated_at, deleted_at, tags) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
                 ON CONFLICT(id) DO UPDATE SET \
                    folder_id=excluded.folder_id, name=excluded.name, \
                    abbreviation=excluded.abbreviation, content_type=excluded.content_type, \
                    body_json=excluded.body_json, triggers_json=excluded.triggers_json, \
                    sensitive=excluded.sensitive, app_scope=excluded.app_scope, \
                    expanded_count=excluded.expanded_count, last_used_at=excluded.last_used_at, \
                    case_mode=excluded.case_mode, created_at=excluded.created_at, \
                    updated_at=excluded.updated_at, deleted_at=excluded.deleted_at, \
                    tags=excluded.tags",
                params![
                    phrase.id,
                    phrase.folder_id,
                    phrase.name,
                    phrase.abbreviation,
                    content_type_str(phrase.content_type),
                    body_json,
                    triggers_json,
                    phrase.sensitive as i64,
                    phrase.app_scope,
                    phrase.expanded_count as i64,
                    phrase.last_used_at,
                    case_mode_str(phrase.case_mode),
                    phrase.created_at,
                    phrase.updated_at,
                    phrase.deleted_at,
                    tags_json,
                ],
            )
            .map_err(db_err)?;
        }
        self.snapshot_current(&phrase.id)?;
        Ok(())
    }

    pub fn get_active_phrase(&self, id: &str) -> DomainResult<Phrase> {
        match self.get_phrase(id)? {
            Some(p) if p.deleted_at.is_none() => Ok(p),
            _ => Err(DomainError::NotFound(format!("phrase {id} not found"))),
        }
    }

    /// Filters applied in Rust (library sizes are modest; avoids dynamic SQL plumbing).
    pub fn list_phrases(
        &self,
        folder_id: Option<&str>,
        q: Option<&str>,
        include_deleted: bool,
    ) -> DomainResult<Vec<Phrase>> {
        let crypto = self.crypto();
        let conn = self.conn.lock().unwrap();
        // deleted_at / folder_id are plaintext columns: filter them in SQL so
        // filtered-out rows never pay the decrypt+parse cost of body_json.
        // prepare_cached: list runs on every library mutation ×3 (index, hotkeys,
        // poller) — a per-call prepare recompiles the SQL each time.
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                        triggers_json, sensitive, app_scope, expanded_count, last_used_at, case_mode, created_at, updated_at, deleted_at, tags \
                 FROM phrases \
                 WHERE (?1 IS NULL OR folder_id = ?1) AND (?2 = 1 OR deleted_at IS NULL)",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![folder_id, include_deleted], |r| row_to_phrase(r, crypto))
            .map_err(db_err)?;
        let needle = q.map(|s| s.to_lowercase());
        let mut out = Vec::new();
        for row in rows {
            let p = row.map_err(db_err)?;
            if let Some(n) = &needle {
                let in_name = p.name.to_lowercase().contains(n);
                let in_abbr = p
                    .abbreviation
                    .as_deref()
                    .map(|a| a.to_lowercase().contains(n))
                    .unwrap_or(false);
                if !in_name && !in_abbr {
                    continue;
                }
            }
            out.push(p);
        }
        Ok(out)
    }

    /// Live phrases by id, in the caller's order — the popup chooser's batch fetch
    /// (resolves 2-5 candidate ids without a whole-library decrypt pass).
    pub fn phrases_by_ids(&self, ids: &[String]) -> DomainResult<Vec<Phrase>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let crypto = self.crypto();
        let conn = self.conn.lock().unwrap();
        let placeholders = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                    triggers_json, sensitive, app_scope, expanded_count, last_used_at, case_mode, created_at, updated_at, deleted_at, tags \
             FROM phrases \
             WHERE deleted_at IS NULL AND id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let list: Vec<&str> = ids.iter().map(String::as_str).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(list.iter()), |r| row_to_phrase(r, crypto))
            .map_err(db_err)?;
        let mut by_id: HashMap<String, Phrase> = HashMap::new();
        for row in rows {
            let p = row.map_err(db_err)?;
            by_id.insert(p.id.clone(), p);
        }
        Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
    }

    pub fn update_phrase(&self, phrase: &Phrase) -> DomainResult<Phrase> {
        phrase.validate()?;
        {
            let conn = self.conn.lock().unwrap();
            if !row_exists(&conn, "SELECT 1 FROM phrases WHERE id = ?1", &phrase.id)? {
                return Err(DomainError::NotFound(format!(
                    "phrase {} not found",
                    phrase.id
                )));
            }
        }
        self.snapshot_current(&phrase.id)?;
        let (body_json, triggers_json, tags_json) = self.encode_phrase_row(phrase)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE phrases SET name=?1, abbreviation=?2, content_type=?3, body_json=?4, \
             triggers_json=?5, sensitive=?6, app_scope=?7, case_mode=?8, updated_at=?9, tags=?10 WHERE id=?11",
            params![
                phrase.name,
                phrase.abbreviation,
                content_type_str(phrase.content_type),
                body_json,
                triggers_json,
                phrase.sensitive as i64,
                phrase.app_scope,
                case_mode_str(phrase.case_mode),
                now_iso(),
                tags_json,
                phrase.id,
            ],
        )
        .map_err(db_err)?;
        Ok(phrase.clone())
    }

    pub fn soft_delete_phrase(&self, id: &str) -> DomainResult<Option<Phrase>> {
        let crypto = self.crypto();
        let conn = self.conn.lock().unwrap();
        let existing = get_phrase_inner(&conn, id, crypto)?;
        match existing {
            None => Ok(None),
            Some(p) if p.deleted_at.is_some() => Ok(None),
            Some(_) => {
                // Bump updated_at along with the tombstone: sync is last-writer-wins
                // on updated_at alone, so a tombstone stamped at the old edit time
                // would lose to the live copies on other devices and never propagate.
                let now = now_iso();
                conn.execute(
                    "UPDATE phrases SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(db_err)?;
                get_phrase_inner(&conn, id, crypto)
            }
        }
    }

    /// Resolution. Deleted phrases never resolve.
    /// First match in storage order wins (hotkey uniqueness not enforced in v1).
    pub fn resolve(&self, trigger: &TriggerRequest) -> DomainResult<Option<Phrase>> {
        let phrases = self.list_phrases(None, None, false)?;
        for phrase in &phrases {
            for binding in &phrase.triggers {
                if binding.kind != trigger.kind {
                    continue;
                }
                match trigger.kind {
                    TriggerType::Autotext => {
                        if trigger.abbreviation.is_some()
                            && binding.abbreviation == trigger.abbreviation
                        {
                            return Ok(Some(phrase.clone()));
                        }
                    }
                    TriggerType::Hotkey => {
                        if trigger.key.is_some() && binding.key == trigger.key {
                            return Ok(Some(phrase.clone()));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    // ----------------------------------------------------------------------- //
    // Clipboard
    // ----------------------------------------------------------------------- //
    /// Insert a clipboard entry, then evict the oldest rows beyond `capacity`.
    /// `capacity == 0` means unlimited (no eviction).
    pub fn add_clipboard(&self, entry: &ClipboardEntry, capacity: usize) -> DomainResult<()> {
        let text = maybe_encrypt(self.crypto(), &entry.text);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (id, text, captured_at) VALUES (?1, ?2, ?3)",
            params![entry.id, text, entry.captured_at],
        )
        .map_err(db_err)?;
        // Capacity-bounded — evict oldest beyond the cap in one statement (no
        // select-all + per-row delete round trip per capture).
        if capacity == 0 {
            return Ok(());
        }
        conn.execute(
            "DELETE FROM clipboard_entries WHERE id NOT IN \
             (SELECT id FROM clipboard_entries ORDER BY captured_at DESC, rowid DESC LIMIT ?1)",
            params![capacity],
        )
        .map_err(db_err)?;
        Ok(())
    }

    pub fn list_clipboard(&self, limit: i64) -> DomainResult<Vec<ClipboardEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, text, captured_at FROM clipboard_entries \
                 ORDER BY captured_at DESC LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(ClipboardEntry {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    captured_at: r.get(2)?,
                })
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let entry: ClipboardEntry = row.map_err(db_err)?;
            // Decrypt; skip locked rows (no key) rather than surfacing ciphertext.
            if let Some(text) = maybe_decrypt(self.crypto(), &entry.text) {
                out.push(ClipboardEntry { text, ..entry });
            }
        }
        Ok(out)
    }

    pub fn clear_clipboard(&self) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_entries", [])
            .map_err(db_err)?;
        Ok(())
    }

    /// Reset: delete all user data (phrases, folders, clipboard, versions). Keeps settings.
    pub fn reset_database(&self) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM phrase_versions; \
             DELETE FROM clipboard_entries; \
             DELETE FROM phrases; \
             DELETE FROM folders;",
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// Idempotent one-time migration: envelope every sensitive phrase body (and the
    /// version snapshots belonging to sensitive phrases), the AI API key, and all clipboard
    /// history — all in one transaction. No-op per row if already enveloped; no-op entirely when
    /// encryption isn't available. Run at startup and whenever the toggle is turned on.
    pub fn migrate_encrypt_at_rest(&self) -> DomainResult<()> {
        let Some(crypto) = self.crypto() else {
            return Ok(());
        };
        if !crypto.is_available() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(db_err)?;

        // Sensitive phrase bodies + their version snapshots + clipboard history. Each batch:
        // collect the rows, drop the statement, then UPDATE (same connection, no active cursor).
        for (select, update) in [
            (
                "SELECT id, body_json FROM phrases WHERE sensitive=1 AND deleted_at IS NULL \
                 AND body_json NOT LIKE 'enc1:%'",
                "UPDATE phrases SET body_json=?1 WHERE id=?2" as &str,
            ),
            (
                "SELECT pv.id, pv.body_json FROM phrase_versions pv \
                 JOIN phrases p ON pv.phrase_id=p.id \
                 WHERE p.sensitive=1 AND pv.body_json NOT LIKE 'enc1:%'",
                "UPDATE phrase_versions SET body_json=?1 WHERE id=?2",
            ),
            (
                "SELECT id, text FROM clipboard_entries WHERE text<>'' AND text NOT LIKE 'enc1:%'",
                "UPDATE clipboard_entries SET text=?1 WHERE id=?2",
            ),
        ] {
            let mut sel = tx.prepare(select).map_err(db_err)?;
            let rows: Vec<(String, String)> = sel
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            drop(sel);
            for (id, raw) in rows {
                let enc = maybe_encrypt(Some(crypto), &raw);
                tx.execute(update, params![enc, id]).map_err(db_err)?;
            }
        }
        // AI API key.
        let key_raw: Option<String> = tx
            .query_row("SELECT ai_api_key FROM settings WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(db_err)?;
        if let Some(k) = key_raw {
            if !k.is_empty() && !k.starts_with(ENVELOPE_PREFIX) {
                let enc = maybe_encrypt(Some(crypto), &k);
                tx.execute("UPDATE settings SET ai_api_key=?1 WHERE id=1", params![enc])
                    .map_err(db_err)?;
            }
        }

        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Inverse of `migrate_encrypt_at_rest`: decrypt every enveloped value back to
    /// plaintext, in one transaction. Used when the user turns the setting off. Rows that can't
    /// be decrypted (no key) are skipped — they stay enveloped (still locked), never corrupted.
    pub fn decrypt_at_rest(&self) -> DomainResult<()> {
        let Some(crypto) = self.crypto() else {
            return Ok(());
        };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(db_err)?;

        for (table, col) in [
            ("phrases", "body_json"),
            ("phrase_versions", "body_json"),
            ("clipboard_entries", "text"),
        ] {
            let select = format!("SELECT id, {col} FROM {table} WHERE {col} LIKE 'enc1:%'");
            let mut sel = tx.prepare(&select).map_err(db_err)?;
            let rows: Vec<(String, String)> = sel
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            drop(sel);
            for (id, raw) in rows {
                if let Some(plain) = crypto.decrypt_str(&raw) {
                    let upd = format!("UPDATE {table} SET {col}=?1 WHERE id=?2");
                    tx.execute(&upd, params![plain, id]).map_err(db_err)?;
                }
            }
        }
        let key_raw: Option<String> = tx
            .query_row("SELECT ai_api_key FROM settings WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(db_err)?;
        if let Some(k) = key_raw {
            if let Some(plain) = crypto.decrypt_str(&k) {
                tx.execute(
                    "UPDATE settings SET ai_api_key=?1 WHERE id=1",
                    params![plain],
                )
                .map_err(db_err)?;
            }
        }

        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Phrase count per folder_id.
    pub fn folder_counts(&self) -> DomainResult<Vec<(String, u32)>> {
        let phrases = self.list_phrases(None, None, false)?;
        let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for p in &phrases {
            *counts.entry(p.folder_id.clone()).or_default() += 1;
        }
        Ok(counts.into_iter().collect())
    }

    /// Build a masked summary for list views.
    pub fn summary(&self, phrase: &Phrase) -> PhraseSummary {
        let preview = if phrase.sensitive {
            "***".to_string()
        } else {
            preview_text(&phrase.body)
        };
        PhraseSummary {
            id: phrase.id.clone(),
            folder_id: phrase.folder_id.clone(),
            name: phrase.name.clone(),
            abbreviation: phrase.abbreviation.clone(),
            content_type: phrase.content_type,
            sensitive: phrase.sensitive,
            content_preview: preview,
            deleted: phrase.deleted_at.is_some(),
            expanded_count: phrase.expanded_count,
            last_used_at: phrase.last_used_at.clone(),
            tags: phrase.tags.clone(),
        }
    }

    /// Usage stats (1.4): bump the expansion counter and stamp last-used. Fire-and-forget
    /// safe — a single fast statement, called from the listener thread after injection.
    pub fn record_usage(&self, phrase_id: &str) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE phrases SET expanded_count = expanded_count + 1, last_used_at = ?1 \
             WHERE id = ?2 AND deleted_at IS NULL",
            params![now_iso(), phrase_id],
        )
        .map_err(db_err)?;
        Ok(())
    }

    // ----------------------------------------------------------------------- //
    // Expansion (Clipboard macro needs the system clipboard)
    // ----------------------------------------------------------------------- //
    /// Expand to (text, image paths) using the live system clipboard. Images
    /// (in body order) are delivered after the text.
    pub async fn expand(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
    ) -> DomainResult<(String, Vec<String>)> {
        let clip = read_clipboard_text();
        self.expand_with_clip(phrase, inputs, clip.as_deref()).await
    }

    /// Expand to the ordered chunk stream — text/image/delay in body order. The
    /// injection worker delivers this; previews and tests use the flat views.
    pub async fn expand_chunks_with_clip(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
        clip: Option<&str>,
    ) -> DomainResult<Vec<OutChunk>> {
        let now = chrono::Utc::now();
        self.expand_inner(phrase, inputs, now, clip, 0).await
    }

    /// Expand with an explicit clipboard value instead of the live system
    /// clipboard — used by the selection-based AI flow (the captured selection
    /// is passed directly) and by tests.
    pub async fn expand_with_clip(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
        clip: Option<&str>,
    ) -> DomainResult<(String, Vec<String>)> {
        let chunks = self.expand_chunks_with_clip(phrase, inputs, clip).await?;
        Ok(OutChunk::flatten(&chunks))
    }

    /// Recursive walk: handles Nested (linked phrases, depth-limited) and Clipboard;
    /// delegates the pure macros to evaluate_macro. Produces the ordered chunk stream
    /// so side-effect macros (Image, Delay) keep their position between text.
    async fn expand_inner(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
        now: chrono::DateTime<chrono::Utc>,
        clip: Option<&str>,
        depth: u8,
    ) -> DomainResult<Vec<OutChunk>> {
        if depth > 8 {
            return Err(DomainError::Validation("phrase nesting too deep".into()));
        }
        let mut out: Vec<OutChunk> = Vec::new();
        for seg in &phrase.body {
            match seg {
                Segment::Text { text } => out.push(OutChunk::Text(text.clone())),
                Segment::Macro { macro_value } => match macro_value {
                    Macro::Clipboard => out.push(OutChunk::Text(clip.unwrap_or("").to_string())),
                    Macro::Image { path } => out.push(OutChunk::Image(path.clone())),
                    Macro::Delay { ms } => out.push(OutChunk::Delay((*ms).clamp(0, 5000))),
                    Macro::Cursor => out.push(OutChunk::Cursor),
                    Macro::Ai { prompt } => {
                        let settings = self.load_settings()?;
                        let resolved = prompt.replace("{clipboard}", clip.unwrap_or(""));
                        let text = crate::triggers::ai::complete(&settings, &resolved).await?;
                        out.push(OutChunk::Text(text));
                    }
                    Macro::AiAction { action_id } => {
                        let action = self.get_ai_action(action_id)?.ok_or_else(|| {
                            DomainError::NotFound(format!("ai action {action_id} not found"))
                        })?;
                        let settings = self.load_settings()?;
                        // {selection} is an alias for {clipboard}: the selection-based AI
                        // flow captures the selection onto the clipboard before expanding.
                        let resolved = action
                            .prompt_template
                            .replace("{clipboard}", clip.unwrap_or(""))
                            .replace("{selection}", clip.unwrap_or(""));
                        let text = crate::triggers::ai::complete(&settings, &resolved).await?;
                        out.push(OutChunk::Text(text));
                    }
                    Macro::Cite { source, style } => {
                        // A blank source prompts once via the inputs popup (field
                        // "cite" — collected by the frontend like an inputform field).
                        let src = if source.trim().is_empty() {
                            inputs
                                .get("cite")
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .ok_or_else(|| DomainError::MissingInput("cite".into()))?
                        } else {
                            source.trim()
                        };
                        let meta = crate::domain::citation::fetch_metadata(src).await?;
                        let text = crate::domain::citation::format_citation(&meta, style)?;
                        out.push(OutChunk::Text(text));
                    }
                    Macro::Nested { phrase_id } => {
                        let linked = self.get_phrase(phrase_id)?.ok_or_else(|| {
                            DomainError::NotFound(format!("nested phrase {phrase_id} not found"))
                        })?;
                        if linked.deleted_at.is_some() {
                            return Err(DomainError::NotFound(format!(
                                "nested phrase {phrase_id} is deleted"
                            )));
                        }
                        // Box the recursive future — an async fn that recurses by value would
                        // otherwise have an infinitely-sized future type.
                        let nested =
                            Box::pin(self.expand_inner(&linked, inputs, now, clip, depth + 1))
                                .await?;
                        out.extend(nested);
                    }
                    Macro::Counter { name } => {
                        out.push(OutChunk::Text(self.next_counter(name)?.to_string()))
                    }
                    other => out.push(OutChunk::Text(evaluate_macro(other, inputs, now)?)),
                },
            }
        }
        Ok(out)
    }

    /// Next value of a named persistent counter (created at 1 on first use). Powers the
    /// counter macro — invoice / ticket numbering that survives restarts.
    pub fn next_counter(&self, name: &str) -> DomainResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "INSERT INTO counters(name, value) VALUES (?1, 1) \
             ON CONFLICT(name) DO UPDATE SET value = value + 1 RETURNING value",
            params![name],
            |r| r.get(0),
        )
        .map_err(db_err)
    }

    /// Group active phrases whose first text segment is identical (duplicate content).
    pub fn find_duplicates(&self) -> DomainResult<Vec<Vec<Phrase>>> {
        let phrases = self.list_phrases(None, None, false)?;
        let mut groups: std::collections::HashMap<String, Vec<Phrase>> =
            std::collections::HashMap::new();
        for p in phrases {
            if let Some(key) = first_text(&p) {
                groups.entry(key).or_default().push(p);
            }
        }
        Ok(groups.into_values().filter(|g| g.len() > 1).collect())
    }

    // ----------------------------------------------------------------------- //
    // Version history
    // ----------------------------------------------------------------------- //
    /// Snapshot the current body+triggers of a phrase into phrase_versions.
    fn snapshot_current(&self, phrase_id: &str) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT body_json, triggers_json FROM phrases WHERE id = ?1",
                params![phrase_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        if let Some((body_json, triggers_json)) = row {
            conn.execute(
                "INSERT INTO phrase_versions (id, phrase_id, body_json, triggers_json, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    phrase_id,
                    body_json,
                    triggers_json,
                    now_iso(),
                ],
            )
            .map_err(db_err)?;
            // Unbounded history used to grow forever (AUDIT 231) — keep the newest
            // MAX_PHRASE_VERSIONS snapshots per phrase.
            conn.execute(
                "DELETE FROM phrase_versions WHERE phrase_id = ?1 AND id NOT IN \
                 (SELECT id FROM phrase_versions WHERE phrase_id = ?1 \
                  ORDER BY saved_at DESC, rowid DESC LIMIT ?2)",
                params![phrase_id, MAX_PHRASE_VERSIONS],
            )
            .map_err(db_err)?;
        }
        Ok(())
    }

    /// Decode a version row's encrypted body + triggers into domain types. `strict`
    /// propagates body parse errors (restore must not silently wipe a phrase);
    /// lenient mode (history list) degrades a corrupt row to empty segments so one
    /// bad row can't blank the list.
    fn decode_version_row(
        &self,
        body_json: &str,
        triggers_json: &str,
        strict: bool,
    ) -> DomainResult<(Vec<Segment>, Vec<TriggerBinding>)> {
        let body_json = decode_body_json(body_json, self.crypto());
        let body: Vec<Segment> = if strict {
            serde_json::from_str(&body_json)
                .map_err(|e| DomainError::Validation(format!("version body: {e}")))?
        } else {
            serde_json::from_str(&body_json).unwrap_or_default()
        };
        let triggers: Vec<TriggerBinding> =
            serde_json::from_str(triggers_json).unwrap_or_default();
        Ok((body, triggers))
    }

    pub fn list_versions(&self, phrase_id: &str) -> DomainResult<Vec<PhraseVersion>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, phrase_id, body_json, triggers_json, saved_at FROM phrase_versions \
                 WHERE phrase_id = ?1 ORDER BY saved_at DESC, rowid DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![phrase_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, pid, body_json, triggers_json, saved_at) = row.map_err(db_err)?;
            let (body, triggers) = self.decode_version_row(&body_json, &triggers_json, false)?;
            out.push(PhraseVersion {
                id,
                phrase_id: pid,
                preview: preview_text(&body),
                body,
                triggers,
                saved_at,
            });
        }
        Ok(out)
    }

    pub fn restore_version(&self, phrase_id: &str, version_id: &str) -> DomainResult<Phrase> {
        let mut phrase = self
            .get_phrase(phrase_id)?
            .ok_or_else(|| DomainError::NotFound(format!("phrase {phrase_id} not found")))?;
        let (body, triggers) = {
            let conn = self.conn.lock().unwrap();
            let row: Option<(String, String)> = conn
                .query_row(
                    "SELECT body_json, triggers_json FROM phrase_versions \
                     WHERE id = ?1 AND phrase_id = ?2",
                    params![version_id, phrase_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            let (bj, tj) = row.ok_or_else(|| DomainError::NotFound("version not found".into()))?;
            self.decode_version_row(&bj, &tj, true)?
        };
        phrase.body = body;
        phrase.triggers = triggers;
        self.update_phrase(&phrase) // snapshots current (pre-restore) + applies
    }

    // ----------------------------------------------------------------------- //
    // Settings
    // ----------------------------------------------------------------------- //
    pub fn load_settings(&self) -> DomainResult<Settings> {
        // Cache-first: the cached copy skips a 26-column SELECT + a keychain
        // decrypt of the API key on every AI macro / AI action / hotkey reload.
        // Writers (`save_settings`, sync's settings merge) clear it.
        if let Ok(g) = self.settings_cache.lock() {
            if let Some(cached) = g.as_ref() {
                return Ok((**cached).clone());
            }
        }
        let fresh = self.load_settings_uncached()?;
        if let Ok(mut g) = self.settings_cache.lock() {
            *g = Some(Arc::new(fresh.clone()));
        }
        Ok(fresh)
    }

    /// Invalidate the settings cache — call after any direct settings-row write.
    pub fn invalidate_settings_cache(&self) {
        if let Ok(mut g) = self.settings_cache.lock() {
            *g = None;
        }
    }

    fn load_settings_uncached(&self) -> DomainResult<Settings> {
        let crypto = self.crypto();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare_cached(
                "SELECT popup_hotkey, autotext_enabled, clipboard_enabled, autostart, \
                        ai_enabled, ai_base_url, ai_api_key, ai_model, \
                        spellcheck_enabled, case_adapt_enabled, spellcheck_ignore, spellcheck_mode, \
                        spellcheck_hotkey, clipboard_max_length, clipboard_max_entries, \
                        spellcheck_exclude_apps, spellcheck_delivery, start_minimized, \
                        encrypt_at_rest, ai_instructions, ai_selection_hotkey, \
                        ai_temperature, ai_max_tokens, \
                        autotext_exclude_apps, sync_dir, review_sentence, sync_passphrase \
                 FROM settings WHERE id = 1",
            )
            .map_err(db_err)?;
        let row = stmt
            .query_row(
                [],
                |r| {
                    let ai_api_key_raw: String = r.get(6)?;
                    Ok(Settings {
                        popup_hotkey: r.get(0)?,
                        autotext_enabled: r.get::<_, i64>(1)? != 0,
                        clipboard_enabled: r.get::<_, i64>(2)? != 0,
                        autostart: r.get::<_, i64>(3)? != 0,
                        ai_enabled: r.get::<_, i64>(4)? != 0,
                        ai_base_url: r.get(5)?,
                        ai_api_key: maybe_decrypt(crypto, &ai_api_key_raw).unwrap_or_default(),
                        ai_model: r.get(7)?,
                        spellcheck_enabled: r.get::<_, i64>(8)? != 0,
                        case_adapt_enabled: r.get::<_, i64>(9)? != 0,
                        spellcheck_ignore: r.get(10)?,
                        spellcheck_mode: r.get(11)?,
                        spellcheck_hotkey: r.get(12)?,
                        clipboard_max_length: r.get::<_, i64>(13)? as u32,
                        clipboard_max_entries: r.get::<_, i64>(14)? as u32,
                        spellcheck_exclude_apps: r.get(15)?,
                        spellcheck_delivery: r.get(16)?,
                        start_minimized: r.get::<_, i64>(17)? != 0,
                        encrypt_at_rest: r.get::<_, i64>(18)? != 0,
                        ai_instructions: r.get(19)?,
                        ai_selection_hotkey: r.get(20)?,
                        ai_temperature: r.get(21)?,
                        ai_max_tokens: r.get(22)?,
                        autotext_exclude_apps: r.get(23)?,
                        sync_dir: r.get(24)?,
                        review_sentence: r.get::<_, i64>(25)? != 0,
                        sync_passphrase: maybe_decrypt(crypto, &r.get::<_, String>(26)?)
                            .unwrap_or_default(),
                    })
                },
            )
            .optional()
            .map_err(db_err)?;
        match row {
            Some(s) => Ok(s),
            None => {
                let s = Settings::defaults();
                conn.execute(
                    "INSERT INTO settings (id, popup_hotkey, autotext_enabled, clipboard_enabled, \
                     autostart, ai_enabled, ai_base_url, ai_api_key, ai_model, \
                     spellcheck_enabled, case_adapt_enabled, spellcheck_ignore, spellcheck_mode, \
                     spellcheck_hotkey, clipboard_max_length, clipboard_max_entries, \
                     spellcheck_exclude_apps, spellcheck_delivery, start_minimized) \
                     VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                    params![
                        s.popup_hotkey,
                        s.autotext_enabled as i64,
                        s.clipboard_enabled as i64,
                        s.autostart as i64,
                        s.ai_enabled as i64,
                        s.ai_base_url,
                        s.ai_api_key,
                        s.ai_model,
                        s.spellcheck_enabled as i64,
                        s.case_adapt_enabled as i64,
                        s.spellcheck_ignore,
                        s.spellcheck_mode,
                        s.spellcheck_hotkey,
                        s.clipboard_max_length as i64,
                        s.clipboard_max_entries as i64,
                        s.spellcheck_exclude_apps,
                        s.spellcheck_delivery,
                        s.start_minimized as i64
                    ],
                )
                .map_err(db_err)?;
                Ok(s)
            }
        }
    }

    pub fn save_settings(&self, s: &Settings) -> DomainResult<Settings> {
        // Don't envelope an empty key (wasteful and confuses the "no key set" check).
        let ai_api_key = if s.ai_api_key.is_empty() {
            String::new()
        } else {
            maybe_encrypt(self.crypto(), &s.ai_api_key)
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (id, popup_hotkey, autotext_enabled, clipboard_enabled, \
             autostart, ai_enabled, ai_base_url, ai_api_key, ai_model, ai_instructions, \
             ai_selection_hotkey, ai_temperature, ai_max_tokens, \
             spellcheck_enabled, case_adapt_enabled, spellcheck_ignore, spellcheck_mode, \
             spellcheck_hotkey, clipboard_max_length, clipboard_max_entries, \
             spellcheck_exclude_apps, spellcheck_delivery, start_minimized, encrypt_at_rest, \
             autotext_exclude_apps, sync_dir, review_sentence, sync_passphrase) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27) \
             ON CONFLICT(id) DO UPDATE SET \
                popup_hotkey=excluded.popup_hotkey, \
                autotext_enabled=excluded.autotext_enabled, \
                clipboard_enabled=excluded.clipboard_enabled, \
                autostart=excluded.autostart, \
                ai_enabled=excluded.ai_enabled, \
                ai_base_url=excluded.ai_base_url, \
                ai_api_key=excluded.ai_api_key, \
                ai_model=excluded.ai_model, \
                ai_instructions=excluded.ai_instructions, \
                ai_selection_hotkey=excluded.ai_selection_hotkey, \
                ai_temperature=excluded.ai_temperature, \
                ai_max_tokens=excluded.ai_max_tokens, \
                autotext_exclude_apps=excluded.autotext_exclude_apps, \
                spellcheck_enabled=excluded.spellcheck_enabled, \
                case_adapt_enabled=excluded.case_adapt_enabled, \
                spellcheck_ignore=excluded.spellcheck_ignore, \
                spellcheck_mode=excluded.spellcheck_mode, \
                spellcheck_hotkey=excluded.spellcheck_hotkey, \
                clipboard_max_length=excluded.clipboard_max_length, \
                clipboard_max_entries=excluded.clipboard_max_entries, \
                spellcheck_exclude_apps=excluded.spellcheck_exclude_apps, \
                spellcheck_delivery=excluded.spellcheck_delivery, \
                start_minimized=excluded.start_minimized, \
                encrypt_at_rest=excluded.encrypt_at_rest, \
                sync_dir=excluded.sync_dir, \
                review_sentence=excluded.review_sentence,                 sync_passphrase=excluded.sync_passphrase",
            params![
                s.popup_hotkey,
                s.autotext_enabled as i64,
                s.clipboard_enabled as i64,
                s.autostart as i64,
                s.ai_enabled as i64,
                s.ai_base_url,
                ai_api_key,
                s.ai_model,
                s.ai_instructions,
                s.ai_selection_hotkey,
                s.ai_temperature,
                s.ai_max_tokens,
                s.spellcheck_enabled as i64,
                s.case_adapt_enabled as i64,
                s.spellcheck_ignore,
                s.spellcheck_mode,
                s.spellcheck_hotkey,
                s.clipboard_max_length as i64,
                s.clipboard_max_entries as i64,
                s.spellcheck_exclude_apps,
                s.spellcheck_delivery,
                s.start_minimized as i64,
                s.encrypt_at_rest as i64,
                s.autotext_exclude_apps,
                s.sync_dir,
                s.review_sentence as i64,
                maybe_encrypt(self.crypto(), &s.sync_passphrase)
            ],
        )
        .map_err(db_err)?;
        self.invalidate_settings_cache();
        Ok(s.clone())
    }

    // ----------------------------------------------------------------------- //
    // AI actions
    // ----------------------------------------------------------------------- //
    fn ai_action_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiAction> {
        Ok(AiAction {
            id: row.get("id")?,
            name: row.get("name")?,
            prompt_template: row.get("prompt_template")?,
            sort_order: row.get("sort_order")?,
            builtin: row.get::<_, i64>("builtin")? != 0,
        })
    }

    pub fn list_ai_actions(&self) -> DomainResult<Vec<AiAction>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, prompt_template, sort_order, builtin FROM ai_actions ORDER BY sort_order, name")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], Self::ai_action_from_row)
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        Ok(rows)
    }

    pub fn get_ai_action(&self, id: &str) -> DomainResult<Option<AiAction>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, prompt_template, sort_order, builtin FROM ai_actions WHERE id = ?1",
            params![id],
            Self::ai_action_from_row,
        )
        .optional()
        .map_err(db_err)
    }

    /// Insert or update an action. Builtin rows may be edited (template/name/order)
    /// but the `builtin` flag itself is preserved from the stored row.
    pub fn save_ai_action(&self, action: &AiAction) -> DomainResult<AiAction> {
        if action.id.trim().is_empty() {
            return Err(DomainError::MissingInput("ai action id is required".into()));
        }
        if action.name.trim().is_empty() {
            return Err(DomainError::MissingInput(
                "ai action name is required".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let stored_builtin: Option<bool> = conn
            .query_row(
                "SELECT builtin FROM ai_actions WHERE id = ?1",
                params![action.id],
                |r| r.get::<_, i64>(0).map(|v| v != 0),
            )
            .optional()
            .map_err(db_err)?;
        let builtin = stored_builtin.unwrap_or(action.builtin);
        conn.execute(
            "INSERT INTO ai_actions (id, name, prompt_template, sort_order, builtin) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
                name=excluded.name, prompt_template=excluded.prompt_template, \
                sort_order=excluded.sort_order",
            params![
                action.id,
                action.name,
                action.prompt_template,
                action.sort_order,
                builtin as i64
            ],
        )
        .map_err(db_err)?;
        Ok(AiAction {
            builtin,
            ..action.clone()
        })
    }

    /// Delete a custom action. Builtin actions can only be edited, never removed —
    /// the seeded set is part of the product's baseline behavior.
    pub fn delete_ai_action(&self, id: &str) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM ai_actions WHERE id = ?1 AND builtin = 0",
                params![id],
            )
            .map_err(db_err)?;
        if n == 0 {
            return Err(DomainError::NotFound(format!(
                "ai action {id} not found or builtin"
            )));
        }
        Ok(())
    }

    /// Idempotently append `word` to the spellcheck-ignore list (stored lowercased).
    /// Read-modify-write under a single connection lock so concurrent callers can't race.
    pub fn add_spellcheck_ignore(&self, word: &str) -> DomainResult<()> {
        let w = word.trim().to_lowercase();
        if w.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        let current: Option<String> = conn
            .query_row(
                "SELECT spellcheck_ignore FROM settings WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let mut words: Vec<String> = current
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if words.iter().any(|x| x == &w) {
            return Ok(()); // already ignored
        }
        words.push(w);
        let joined = words.join(", ");
        conn.execute(
            "UPDATE settings SET spellcheck_ignore = ?1 WHERE id = 1",
            params![joined],
        )
        .map_err(db_err)?;
        self.invalidate_settings_cache();
        Ok(())
    }

    /// Record a learned spellcheck correction (typo → correction). Upsert: a re-learned
    /// typo overwrites the previous correction. Learned on accept, removed on undo.
    pub fn add_spellcheck_learned(&self, typo: &str, correction: &str) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO spellcheck_learned (typo, correction) VALUES (?1, ?2) \
             ON CONFLICT(typo) DO UPDATE SET correction = excluded.correction",
            params![typo.trim().to_lowercase(), correction.trim().to_lowercase()],
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// Remove a learned correction (the user undid it — the pair was wrong).
    pub fn remove_spellcheck_learned(&self, typo: &str) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM spellcheck_learned WHERE typo = ?1",
            params![typo.trim().to_lowercase()],
        )
        .map_err(db_err)?;
        Ok(())
    }

    /// All learned corrections (typo → correction), loaded into the runtime map at startup.
    pub fn list_spellcheck_learned(
        &self,
    ) -> DomainResult<std::collections::HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT typo, correction FROM spellcheck_learned")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (t, c) = row.map_err(db_err)?;
            out.insert(t, c);
        }
        Ok(out)
    }
}

fn row_to_folder(r: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        name: r.get(1)?,
        parent_id: r.get(2)?,
        created_at: r.get(3)?,
        enabled: r.get::<_, i64>(4)? != 0,
    })
}

fn row_to_phrase(r: &rusqlite::Row, crypto: Option<&Crypto>) -> rusqlite::Result<Phrase> {
    let body_json: String = r.get(5)?;
    let triggers_json: String = r.get(6)?;
    let content_type: String = r.get(4)?;
    let tags_json: String = r.get(15)?;
    let body_json = decode_body_json(&body_json, crypto);
    let body: Vec<Segment> = serde_json::from_str(&body_json).unwrap_or_default();
    let triggers: Vec<TriggerBinding> = serde_json::from_str(&triggers_json).unwrap_or_default();
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Phrase {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        name: r.get(2)?,
        abbreviation: r.get(3)?,
        content_type: parse_content_type(&content_type),
        body,
        triggers,
        sensitive: r.get::<_, i64>(7)? != 0,
        app_scope: r.get(8)?,
        expanded_count: r.get::<_, i64>(9)?.max(0) as u32,
        last_used_at: r.get(10)?,
        case_mode: parse_case_mode(&r.get::<_, String>(11)?),
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        deleted_at: r.get(14)?,
        tags,
    })
}

fn get_phrase_inner(
    conn: &Connection,
    id: &str,
    crypto: Option<&Crypto>,
) -> DomainResult<Option<Phrase>> {
    let row = conn
        .query_row(
            "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                    triggers_json, sensitive, app_scope, expanded_count, last_used_at, case_mode, created_at, updated_at, deleted_at, tags \
             FROM phrases WHERE id = ?1",
            params![id],
            |r| row_to_phrase(r, crypto),
        )
        .optional()
        .map_err(db_err)?;
    Ok(row)
}

/// Walk up the parent chain; reaching ourselves means a cycle.
/// (Defensive — unreachable via the create-only API.)
fn guard_no_cycle(conn: &Connection, folder_id: &str, parent_id: &str) -> DomainResult<()> {
    let mut cursor = parent_id.to_string();
    let mut seen = std::collections::HashSet::new();
    loop {
        if cursor.as_str() == folder_id || seen.contains(&cursor) {
            return Err(DomainError::Cycle(
                "folder parent_id would create a cycle".into(),
            ));
        }
        seen.insert(cursor.clone());
        let next: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM folders WHERE id = ?1",
                params![cursor],
                |r| r.get::<_, Option<String>>(0),
            )
            .map_err(db_err)?;
        match next {
            Some(p) => cursor = p,
            None => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Macro;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn counter_increments_per_name_and_persists() {
        let db = db();
        assert_eq!(db.next_counter("inv").unwrap(), 1);
        assert_eq!(db.next_counter("inv").unwrap(), 2);
        assert_eq!(db.next_counter("other").unwrap(), 1); // separate sequence
        assert_eq!(db.next_counter("inv").unwrap(), 3);
    }

    #[test]
    fn add_spellcheck_ignore_persists_idempotently() {
        let db = db();
        let _ = db.load_settings().unwrap(); // ensure the settings row exists
        db.add_spellcheck_ignore("Frontline").unwrap();
        db.add_spellcheck_ignore("frontline").unwrap(); // case-insensitive duplicate
        db.add_spellcheck_ignore("nginx").unwrap();
        let s = db.load_settings().unwrap();
        let words: Vec<&str> = s.spellcheck_ignore.split(',').map(|w| w.trim()).collect();
        assert!(words.contains(&"frontline"), "stored lowercased");
        assert!(words.contains(&"nginx"));
        assert_eq!(
            words.iter().filter(|w| **w == "frontline").count(),
            1,
            "idempotent"
        );
    }

    #[test]
    fn spellcheck_learned_roundtrip_and_unlearn() {
        let db = db();
        let _ = db.load_settings().unwrap(); // ensure settings row / migrations ran
        db.add_spellcheck_learned("recieve", "receive").unwrap();
        // upsert: re-learn overwrites
        db.add_spellcheck_learned("recieve", "receiver").unwrap();
        let learned = db.list_spellcheck_learned().unwrap();
        assert_eq!(learned.get("recieve").map(String::as_str), Some("receiver"));
        // unlearn
        db.remove_spellcheck_learned("recieve").unwrap();
        assert!(!db
            .list_spellcheck_learned()
            .unwrap()
            .contains_key("recieve"));
        // removing a non-existent typo is a no-op, not an error
        db.remove_spellcheck_learned("zzzz").unwrap();
    }

    fn new_folder(db: &Db, name: &str) -> Folder {
        let f = Folder {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            parent_id: None,
            created_at: now_iso(),
            enabled: true,
        };
        db.create_folder(&f).unwrap()
    }

    fn new_phrase(db: &Db, folder_id: &str, name: &str, body: Vec<Segment>) -> Phrase {
        let p = Phrase {
            id: uuid::Uuid::new_v4().to_string(),
            folder_id: folder_id.into(),
            name: name.into(),
            abbreviation: None,
            content_type: ContentType::Plain,
            body,
            triggers: vec![],
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: now_iso(),
            updated_at: now_iso(),
            deleted_at: None,
            tags: Vec::new(),
        };
        db.create_phrase(&p).unwrap()
    }

    fn clip(text: &str, i: u32) -> ClipboardEntry {
        // distinct, strictly-increasing timestamps (i as seconds from a base) so
        // "oldest by captured_at" is well-defined for the eviction test.
        let base = chrono::Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
        ClipboardEntry {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.into(),
            captured_at: (base + chrono::Duration::seconds(i as i64)).to_rfc3339(),
        }
    }

    #[test]
    fn phrase_crud_roundtrip() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let p = new_phrase(&db, &f.id, "Sig", vec![Segment::Text { text: "Hi".into() }]);
        let got = db.get_phrase(&p.id).unwrap().unwrap();
        assert_eq!(got.name, "Sig");
        assert_eq!(db.list_phrases(None, None, false).unwrap().len(), 1);
    }

    #[test]
    fn duplicate_abbreviation_allowed_for_multimatch() {
        // Multiple phrases may share an abbreviation (resolved by a selection menu).
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut a = new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "a".into() }]);
        a.abbreviation = Some("sig".into());
        a.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("sig".into()),
            key: None,
            regex: false,
            execute: Default::default(),
        }];
        db.update_phrase(&a).unwrap();
        let mut b = new_phrase(&db, &f.id, "B", vec![Segment::Text { text: "b".into() }]);
        b.abbreviation = Some("sig".into());
        // no Conflict — both coexist (set via update since new_phrase already inserted b)
        db.update_phrase(&b).unwrap();
        let matches = db
            .list_phrases(None, Some("sig"), false)
            .unwrap()
            .iter()
            .filter(|p| p.abbreviation.as_deref() == Some("sig"))
            .count();
        assert_eq!(matches, 2);
    }

    #[test]
    fn soft_delete_excludes_from_resolve_and_list() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut p = new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "x".into() }]);
        p.abbreviation = Some("sig".into());
        p.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("sig".into()),
            key: None,
            regex: false,
            execute: Default::default(),
        }];
        db.update_phrase(&p).unwrap();
        db.soft_delete_phrase(&p.id).unwrap();
        assert!(db.list_phrases(None, None, false).unwrap().is_empty());
        assert_eq!(db.list_phrases(None, None, true).unwrap().len(), 1);
        let trig = TriggerRequest {
            kind: TriggerType::Autotext,
            abbreviation: Some("sig".into()),
            key: None,
        };
        assert!(db.resolve(&trig).unwrap().is_none());
    }

    #[test]
    fn soft_delete_and_folder_merge_bump_updated_at() {
        // Sync is LWW on updated_at alone: tombstones and folder moves that keep the
        // old stamp lose to live copies on other devices and never propagate.
        let db = db();
        let f1 = new_folder(&db, "Inbox");
        let f2 = new_folder(&db, "Work");
        let p = new_phrase(&db, &f1.id, "A", vec![Segment::Text { text: "x".into() }]);
        let before = p.updated_at.clone();

        db.merge_folders(&f1.id, &f2.id).unwrap();
        let moved = db.get_phrase(&p.id).unwrap().unwrap();
        assert_eq!(moved.folder_id, f2.id);
        assert!(moved.updated_at > before, "merge_folders must bump updated_at");

        let after_move = moved.updated_at.clone();
        db.soft_delete_phrase(&p.id).unwrap();
        let tombstone = db.get_phrase(&p.id).unwrap().unwrap();
        assert!(tombstone.deleted_at.is_some());
        assert!(
            tombstone.updated_at > after_move,
            "soft_delete_phrase must bump updated_at"
        );
    }

    #[test]
    fn folder_not_empty_blocks_delete() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "x".into() }]);
        let err = db.delete_folder(&f.id).unwrap_err();
        assert!(matches!(err, DomainError::FolderNotEmpty(_)));
    }

    #[test]
    fn merge_folders_moves_all_phrases_and_deletes_source() {
        let db = db();
        let src = new_folder(&db, "Old");
        let dst = new_folder(&db, "New");
        let live = new_phrase(
            &db,
            &src.id,
            "kept",
            vec![Segment::Text { text: "k".into() }],
        );
        // A soft-deleted phrase must move too — an undelete must not land in a ghost folder.
        let mut gone = new_phrase(
            &db,
            &src.id,
            "gone",
            vec![Segment::Text { text: "g".into() }],
        );
        gone.deleted_at = Some(now_iso());
        db.soft_delete_phrase(&gone.id).unwrap();

        let moved = db.merge_folders(&src.id, &dst.id).unwrap();
        assert_eq!(moved, Some(2));

        // source folder is gone
        assert!(!db.list_folders().unwrap().iter().any(|f| f.id == src.id));
        // live phrase now resolves under the target folder
        let p = db.get_phrase(&live.id).unwrap().unwrap();
        assert_eq!(p.folder_id, dst.id);
        // soft-deleted row moved as well (raw check — it's invisible via get_phrase)
        let fid: String = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT folder_id FROM phrases WHERE id = ?1",
                params![gone.id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(fid, dst.id);
        // no version snapshots were written for the move (content unchanged)
        assert!(db.list_versions(&live.id).unwrap().is_empty());
        // merging into a now-deleted folder reports the miss
        assert_eq!(db.merge_folders(&src.id, &dst.id).unwrap(), None);
    }

    #[test]
    fn record_usage_increments_and_stamps_and_survives_update() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let p = new_phrase(&db, &f.id, "Sig", vec![Segment::Text { text: "Hi".into() }]);

        assert_eq!(db.get_phrase(&p.id).unwrap().unwrap().expanded_count, 0);
        db.record_usage(&p.id).unwrap();
        db.record_usage(&p.id).unwrap();
        let got = db.get_phrase(&p.id).unwrap().unwrap();
        assert_eq!(got.expanded_count, 2);
        assert!(got.last_used_at.is_some());

        // Editing the phrase must NOT reset the counter.
        let edited = Phrase {
            name: "Sig v2".into(),
            ..got
        };
        db.update_phrase(&edited).unwrap();
        let after = db.get_phrase(&p.id).unwrap().unwrap();
        assert_eq!(after.expanded_count, 2);
        assert_eq!(after.name, "Sig v2");

        // Soft-deleted phrases are not counted.
        db.soft_delete_phrase(&p.id).unwrap();
        db.record_usage(&p.id).unwrap();
        assert_eq!(db.get_phrase(&p.id).unwrap().unwrap().expanded_count, 2);

        // Summary carries the stats for list views.
        assert_eq!(db.summary(&after).expanded_count, 2);
    }

    #[test]
    fn merge_folders_rejects_self_missing_target() {
        let db = db();
        let f = new_folder(&db, "A");
        assert!(matches!(
            db.merge_folders(&f.id, &f.id).unwrap_err(),
            DomainError::Validation(_)
        ));
        let other = new_folder(&db, "B");
        let ghost_id = format!("{}-nope", other.id);
        assert!(matches!(
            db.merge_folders(&f.id, &ghost_id).unwrap_err(),
            DomainError::NotFound(_)
        ));
    }

    #[test]
    fn clipboard_capacity_evicts_oldest() {
        const CAP: usize = 100; // the default capacity
        let db = db();
        for i in 0..(CAP as u32 + 1) {
            db.add_clipboard(&clip(&format!("item-{i}"), i), CAP)
                .unwrap();
        }
        let listed = db.list_clipboard((CAP as i64) + 1).unwrap();
        assert_eq!(listed.len(), CAP);
        assert!(listed.iter().all(|e| e.text != "item-0"));
        assert!(listed.iter().any(|e| e.text == format!("item-{CAP}")));
    }

    #[test]
    fn clipboard_capacity_is_configurable() {
        let db = db();
        // A custom (smaller) cap evicts the oldest beyond it.
        for i in 0..10u32 {
            db.add_clipboard(&clip(&format!("item-{i}"), i), 5).unwrap();
        }
        let listed = db.list_clipboard(100).unwrap();
        assert_eq!(listed.len(), 5);
        // Most recent 5 kept (item-9 .. item-5); item-0..4 evicted.
        assert_eq!(listed.first().unwrap().text, "item-9");
        assert!(listed
            .iter()
            .all(|e| !e.text.ends_with("-0") && !e.text.ends_with("-4")));
    }

    #[test]
    fn clipboard_capacity_zero_is_unlimited() {
        let db = db();
        for i in 0..150u32 {
            db.add_clipboard(&clip(&format!("item-{i}"), i), 0).unwrap();
        }
        let listed = db.list_clipboard(1000).unwrap();
        assert_eq!(listed.len(), 150);
    }

    #[test]
    fn expand_through_repo_resolve() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut p = new_phrase(
            &db,
            &f.id,
            "Greet",
            vec![
                Segment::Text {
                    text: "Hello ".into(),
                },
                Segment::Macro {
                    macro_value: Macro::InputForm {
                        field: "n".into(),
                        kind: Default::default(),
                        options: vec![],
                    },
                },
            ],
        );
        p.abbreviation = Some("hi".into());
        p.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("hi".into()),
            key: None,
            regex: false,
            execute: Default::default(),
        }];
        db.update_phrase(&p).unwrap();
        let trig = TriggerRequest {
            kind: TriggerType::Autotext,
            abbreviation: Some("hi".into()),
            key: None,
        };
        let phrase = db.resolve(&trig).unwrap().unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("n".into(), "Sam".into());
        assert_eq!(
            tauri::async_runtime::block_on(db.expand(&phrase, &inputs))
                .unwrap()
                .0,
            "Hello Sam"
        );
    }

    #[test]
    fn nested_phrase_expands_recursively() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let inner = new_phrase(
            &db,
            &f.id,
            "Inner",
            vec![Segment::Text {
                text: "World".into(),
            }],
        );
        let outer = new_phrase(
            &db,
            &f.id,
            "Outer",
            vec![
                Segment::Text {
                    text: "Hello ".into(),
                },
                Segment::Macro {
                    macro_value: Macro::Nested {
                        phrase_id: inner.id.clone(),
                    },
                },
            ],
        );
        assert_eq!(
            tauri::async_runtime::block_on(db.expand(&outer, &HashMap::new()))
                .unwrap()
                .0,
            "Hello World"
        );
    }

    #[test]
    fn find_duplicates_groups_identical_content() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        new_phrase(
            &db,
            &f.id,
            "A",
            vec![Segment::Text {
                text: "same text".into(),
            }],
        );
        new_phrase(
            &db,
            &f.id,
            "B",
            vec![Segment::Text {
                text: "same text".into(),
            }],
        );
        new_phrase(
            &db,
            &f.id,
            "C",
            vec![Segment::Text {
                text: "unique".into(),
            }],
        );
        let groups = db.find_duplicates().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
    }

    #[test]
    fn version_history_records_and_restores() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut p = new_phrase(&db, &f.id, "P", vec![Segment::Text { text: "v1".into() }]);
        // edit -> snapshots "v1", applies "v2"
        p.body = vec![Segment::Text { text: "v2".into() }];
        db.update_phrase(&p).unwrap();

        let versions = db.list_versions(&p.id).unwrap();
        assert_eq!(versions.len(), 1);
        // restoring the snapshot brings back "v1"
        let restored = db.restore_version(&p.id, &versions[0].id).unwrap();
        match &restored.body[0] {
            Segment::Text { text } => assert_eq!(text, "v1"),
            _ => panic!("expected text segment"),
        }
    }

    #[test]
    fn reserved_content_type_rejected() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut p = new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "x".into() }]);
        p.content_type = ContentType::Html;
        let err = db.create_phrase(&p).unwrap_err();
        assert!(matches!(err, DomainError::ReservedContentType(_)));
    }

    // ----- encryption-at-rest -------------------------------------------------- //

    const TEST_DEK: [u8; 32] = [0x42; 32];

    fn db_with_crypto() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.set_crypto(Arc::new(Crypto::for_testing(Some(TEST_DEK))));
        db
    }

    fn sensitive_phrase(db: &Db, folder_id: &str, name: &str, body: &str) -> Phrase {
        let p = Phrase {
            id: uuid::Uuid::new_v4().to_string(),
            folder_id: folder_id.into(),
            name: name.into(),
            abbreviation: None,
            content_type: ContentType::Plain,
            body: vec![Segment::Text { text: body.into() }],
            triggers: vec![],
            sensitive: true,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: now_iso(),
            updated_at: now_iso(),
            deleted_at: None,
            tags: Vec::new(),
        };
        db.create_phrase(&p).unwrap()
    }

    fn raw_col(db: &Db, table: &str, col: &str, id: &str) -> String {
        let conn = db.conn.lock().unwrap();
        let sql = format!("SELECT {col} FROM {table} WHERE id = ?1");
        conn.query_row(&sql, params![id], |r| r.get::<_, String>(0))
            .unwrap()
    }

    #[test]
    fn sensitive_body_is_encrypted_at_rest() {
        let db = db_with_crypto();
        let f = new_folder(&db, "Inbox");
        let p = sensitive_phrase(&db, &f.id, "api", "sk-secret-123");
        // On disk: enveloped, not the plaintext.
        let raw = raw_col(&db, "phrases", "body_json", &p.id);
        assert!(
            raw.starts_with(ENVELOPE_PREFIX),
            "raw body should be enveloped"
        );
        assert!(!raw.contains("sk-secret-123"));
        // Through the API: transparently decrypted.
        let fetched = db.get_phrase(&p.id).unwrap().unwrap();
        assert_eq!(
            fetched.body,
            vec![Segment::Text {
                text: "sk-secret-123".into()
            }]
        );
    }

    #[test]
    fn nonsensitive_body_stays_plaintext() {
        let db = db_with_crypto();
        let f = new_folder(&db, "Inbox");
        let p = new_phrase(
            &db,
            &f.id,
            "hi",
            vec![Segment::Text {
                text: "hello".into(),
            }],
        );
        let raw = raw_col(&db, "phrases", "body_json", &p.id);
        assert!(
            !raw.starts_with(ENVELOPE_PREFIX),
            "non-sensitive body stays plaintext"
        );
        assert!(raw.contains("hello"));
    }

    #[test]
    fn no_crypto_layer_means_plaintext() {
        // The default in-memory db (no set_crypto) is pure plaintext.
        let db = db();
        let f = new_folder(&db, "Inbox");
        let p = sensitive_phrase(&db, &f.id, "api", "sk-xyz");
        let raw = raw_col(&db, "phrases", "body_json", &p.id);
        assert!(
            !raw.starts_with(ENVELOPE_PREFIX),
            "no crypto → plaintext even if sensitive"
        );
        assert!(raw.contains("sk-xyz"));
    }

    #[test]
    fn version_snapshot_carries_envelope_verbatim() {
        let db = db_with_crypto();
        let f = new_folder(&db, "Inbox");
        let mut p = sensitive_phrase(&db, &f.id, "api", "v1-secret");
        // Updating snapshots the current body into phrase_versions (raw envelope copy).
        p.body = vec![Segment::Text {
            text: "v2-secret".into(),
        }];
        db.update_phrase(&p).unwrap();

        let versions = db.list_versions(&p.id).unwrap();
        assert_eq!(versions.len(), 1);
        // list_versions decrypts for display.
        assert_eq!(
            versions[0].body,
            vec![Segment::Text {
                text: "v1-secret".into()
            }]
        );

        // The stored snapshot column is enveloped (rode through the raw copy untouched).
        let conn = db.conn.lock().unwrap();
        let stored: String = conn
            .query_row(
                "SELECT body_json FROM phrase_versions WHERE phrase_id=?1",
                params![p.id],
                |r| r.get(0),
            )
            .unwrap();
        drop(conn);
        assert!(stored.starts_with(ENVELOPE_PREFIX));
    }

    #[test]
    fn clipboard_text_is_encrypted_at_rest() {
        let db = db_with_crypto();
        let entry = clip("confidential", 1);
        db.add_clipboard(&entry, 100).unwrap();
        let raw = raw_col(&db, "clipboard_entries", "text", &entry.id);
        assert!(raw.starts_with(ENVELOPE_PREFIX));
        assert!(!raw.contains("confidential"));
        // list_clipboard decrypts transparently.
        let listed = db.list_clipboard(100).unwrap();
        assert_eq!(listed[0].text, "confidential");
    }

    #[test]
    fn ai_generation_settings_roundtrip() {
        let db = db();
        let mut s = db.load_settings().unwrap();
        s.ai_temperature = 0.4;
        s.ai_max_tokens = 1024;
        db.save_settings(&s).unwrap();
        let loaded = db.load_settings().unwrap();
        assert!((loaded.ai_temperature - 0.4).abs() < 1e-9);
        assert_eq!(loaded.ai_max_tokens, 1024);
        // Back to the defaults (1.0 / unset) persists too.
        s.ai_temperature = 1.0;
        s.ai_max_tokens = 0;
        db.save_settings(&s).unwrap();
        let loaded = db.load_settings().unwrap();
        assert!((loaded.ai_temperature - 1.0).abs() < 1e-9);
        assert_eq!(loaded.ai_max_tokens, 0);
    }

    #[test]
    fn ai_api_key_encrypted_when_nonempty() {
        let db = db_with_crypto();
        let mut s = db.load_settings().unwrap();
        s.ai_api_key = "sk-key-999".into();
        db.save_settings(&s).unwrap();
        let conn = db.conn.lock().unwrap();
        let raw: String = conn
            .query_row("SELECT ai_api_key FROM settings WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        drop(conn);
        assert!(
            raw.starts_with(ENVELOPE_PREFIX),
            "non-empty key is enveloped"
        );
        // Empty key is NOT enveloped.
        s.ai_api_key = String::new();
        db.save_settings(&s).unwrap();
        let conn = db.conn.lock().unwrap();
        let raw2: String = conn
            .query_row("SELECT ai_api_key FROM settings WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        drop(conn);
        assert!(raw2.is_empty(), "empty key stays empty");
        // Loads back as the plaintext key.
        s.ai_api_key = "sk-key-999".into();
        db.save_settings(&s).unwrap();
        assert_eq!(db.load_settings().unwrap().ai_api_key, "sk-key-999");
    }

    #[test]
    fn migrate_encrypt_at_rest_is_idempotent() {
        // Start plaintext (no crypto), create a sensitive phrase + clipboard + ai key.
        let db = db();
        let _ = db.load_settings().unwrap();
        let f = new_folder(&db, "Inbox");
        let p = sensitive_phrase(&db, &f.id, "api", "sk-plain");
        let entry = clip("pw", 1);
        db.add_clipboard(&entry, 100).unwrap();
        let mut s = db.load_settings().unwrap();
        s.ai_api_key = "sk-ai".into();
        db.save_settings(&s).unwrap();

        // Attach crypto and migrate.
        db.set_crypto(Arc::new(Crypto::for_testing(Some(TEST_DEK))));
        db.migrate_encrypt_at_rest().unwrap();
        assert!(raw_col(&db, "phrases", "body_json", &p.id).starts_with(ENVELOPE_PREFIX));
        assert!(raw_col(&db, "clipboard_entries", "text", &entry.id).starts_with(ENVELOPE_PREFIX));
        // Second run is a no-op (no double-prefix, no error).
        db.migrate_encrypt_at_rest().unwrap();
        let body = raw_col(&db, "phrases", "body_json", &p.id);
        assert_eq!(
            body.matches(ENVELOPE_PREFIX).count(),
            1,
            "no double encryption"
        );
        // And the data still decrypts.
        assert_eq!(
            db.get_phrase(&p.id).unwrap().unwrap().body,
            vec![Segment::Text {
                text: "sk-plain".into()
            }]
        );
    }

    #[test]
    fn decrypt_at_rest_round_trips_to_plaintext() {
        let db = db_with_crypto();
        let f = new_folder(&db, "Inbox");
        let p = sensitive_phrase(&db, &f.id, "api", "sk-rt");
        db.decrypt_at_rest().unwrap();
        let body = raw_col(&db, "phrases", "body_json", &p.id);
        assert!(
            !body.starts_with(ENVELOPE_PREFIX),
            "decrypted back to plaintext"
        );
        assert!(body.contains("sk-rt"));
        // Reads still work (plaintext passthrough with crypto attached).
        assert_eq!(
            db.get_phrase(&p.id).unwrap().unwrap().body,
            vec![Segment::Text {
                text: "sk-rt".into()
            }]
        );
    }

    #[test]
    fn locked_phrase_degrades_to_placeholder() {
        // Encrypt a body under key A, then try to read it with a different key (keychain loss /
        // new machine). It must degrade to a placeholder, never panic or leak ciphertext.
        let db = Db::open_in_memory().unwrap();
        db.set_crypto(Arc::new(Crypto::for_testing(Some(TEST_DEK))));
        let f = new_folder(&db, "Inbox");
        let p = sensitive_phrase(&db, &f.id, "api", "sk-real");
        let envelope = raw_col(&db, "phrases", "body_json", &p.id);
        assert!(envelope.starts_with(ENVELOPE_PREFIX));

        let wrong_key = Crypto::for_testing(Some([0x99; 32]));
        let decoded = decode_body_json(&envelope, Some(&wrong_key));
        assert!(
            decoded.contains(LOCKED_PLACEHOLDER),
            "wrong key → placeholder body"
        );
        assert!(!decoded.contains("sk-real"));

        // The correct key still decrypts it.
        let decoded_ok = decode_body_json(&envelope, Some(&Crypto::for_testing(Some(TEST_DEK))));
        assert!(decoded_ok.contains("sk-real"));
    }
    // -- AI actions (1.6) ------------------------------------------------------- //
    #[test]
    fn builtin_ai_actions_are_seeded_and_editable() {
        let db = db();
        let actions = db.list_ai_actions().unwrap();
        assert_eq!(
            actions.len(),
            crate::domain::models::BUILTIN_AI_ACTIONS.len()
        );
        assert!(actions.iter().all(|a| a.builtin));

        // Edit a builtin: name/template change, builtin flag preserved.
        let mut translated = actions
            .iter()
            .find(|a| a.id == "ai-translate")
            .unwrap()
            .clone();
        translated.name = "Translate (DE)".into();
        translated.prompt_template = "Translate to German:\n{clipboard}".into();
        let saved = db.save_ai_action(&translated).unwrap();
        assert!(saved.builtin);
        assert_eq!(
            db.get_ai_action("ai-translate").unwrap().unwrap().name,
            "Translate (DE)"
        );

        // Builtins cannot be deleted.
        assert!(db.delete_ai_action("ai-translate").is_err());
        assert!(db.get_ai_action("ai-translate").unwrap().is_some());
    }

    #[test]
    fn merged_builtin_ai_actions_are_retired_unless_edited() {
        let db = db();
        // Fresh installs never seed the retired (merged-into-a-sibling) ids at all.
        for (retired, ..) in crate::domain::models::MERGED_AI_ACTIONS {
            assert!(db.get_ai_action(retired).unwrap().is_none(), "{retired}");
        }

        // Simulate a pre-merge database: both retired builtins present — one untouched,
        // one renamed/rewritten by the user.
        {
            let conn = db.conn.lock().unwrap();
            for (id, name, template) in [
                (
                    "ai-clarity",
                    "Rewrite for clarity",
                    "Rewrite for maximum clarity and precision:\n{clipboard}",
                ),
                (
                    "ai-simplify",
                    "My simplifier",
                    "Simplify this my way:\n{clipboard}",
                ),
            ] {
                conn.execute(
                    "INSERT INTO ai_actions (id, name, prompt_template, sort_order, builtin) \
                 VALUES (?1, ?2, ?3, 90, 1)",
                    params![id, name, template],
                )
                .unwrap();
            }
        }
        // Re-running migrations (what the next launch does) retires the untouched row
        // and leaves the user-edited one alone.
        db.run_migrations().unwrap();
        assert!(db.get_ai_action("ai-clarity").unwrap().is_none());
        let kept = db.get_ai_action("ai-simplify").unwrap().unwrap();
        assert_eq!(kept.name, "My simplifier");
    }

    #[test]
    fn retemplated_builtin_ai_actions_upgrade_in_place() {
        let db = db();
        // A pre-upgrade DB still carries the old French-only Translate template.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM ai_actions WHERE id = 'ai-translate'", [])
                .unwrap();
            conn.execute(
                "INSERT INTO ai_actions (id, name, prompt_template, sort_order, builtin) \
             VALUES ('ai-translate', 'Translate', 'Translate to French:\n{clipboard}', 0, 1)",
                [],
            )
            .unwrap();
        }
        db.run_migrations().unwrap();
        // Untouched template → upgraded through the whole chain to the CURRENT
        // builtin text ({language} prompt, v2 wording).
        let upgraded = db.get_ai_action("ai-translate").unwrap().unwrap();
        let builtin_v2 = crate::domain::models::BUILTIN_AI_ACTIONS
            .iter()
            .find(|(id, _, _)| *id == "ai-translate")
            .map(|(_, _, t)| *t)
            .unwrap();
        assert_eq!(upgraded.prompt_template, builtin_v2);

        // A user-edited template is left exactly as-is.
        db.save_ai_action(&AiAction {
            prompt_template: "Translate to German:\n{clipboard}".into(),
            ..upgraded
        })
        .unwrap();
        db.run_migrations().unwrap();
        assert_eq!(
            db.get_ai_action("ai-translate")
                .unwrap()
                .unwrap()
                .prompt_template,
            "Translate to German:\n{clipboard}"
        );
    }

    #[test]
    fn revamp_converges_untouched_builtins_and_retires_dropped_ones() {
        let db = db();
        // Simulate a pre-revamp database: stock rows (old text), a user-edited
        // row, and a builtin the revamp retires.
        {
            let conn = db.conn.lock().unwrap();
            for (id, name, template, order) in [
                (
                    "ai-formal",
                    "Make formal",
                    "Rewrite in a formal, professional tone. Keep the meaning and roughly the same length:\n{clipboard}",
                    1,
                ),
                (
                    "ai-translate",
                    "My translator",
                    "Translate to German:\n{clipboard}",
                    0,
                ),
                (
                    "ai-paraphrase",
                    "Paraphrase",
                    "Paraphrase this text in a {mode} style — same meaning, fresh wording, similar length:\n{clipboard}",
                    19,
                ),
            ] {
                conn.execute(
                    "INSERT OR REPLACE INTO ai_actions (id, name, prompt_template, sort_order, builtin) \
                     VALUES (?1, ?2, ?3, ?4, 1)",
                    params![id, name, template, order],
                )
                .unwrap();
            }
        }
        db.run_migrations().unwrap();

        // Untouched stock row → retitled, re-prompted, re-sorted onto the menu.
        let formal = db.get_ai_action("ai-formal").unwrap().unwrap();
        let builtin = crate::domain::models::BUILTIN_AI_ACTIONS
            .iter()
            .find(|(id, _, _)| *id == "ai-formal")
            .unwrap();
        assert_eq!(formal.name, builtin.1);
        assert_eq!(formal.prompt_template, builtin.2);
        assert_eq!(formal.sort_order, 2); // Grammar check, Rewrite for clarity, then this.

        // User-edited template keeps its text (name too) and its slot.
        let mine = db.get_ai_action("ai-translate").unwrap().unwrap();
        assert_eq!(mine.name, "My translator");
        assert_eq!(mine.prompt_template, "Translate to German:\n{clipboard}");
        assert_eq!(mine.sort_order, 0);

        // Retired builtin, untouched → deleted by the revamp.
        assert!(db.get_ai_action("ai-paraphrase").unwrap().is_none());

        // New revamp builtins (no pre-revamp row) are seeded in menu order.
        let humanize = db.get_ai_action("ai-humanize").unwrap().unwrap();
        assert_eq!(humanize.sort_order, 4);
    }

    #[test]
    fn custom_ai_actions_crud_roundtrip() {
        let db = db();
        assert!(db.list_ai_actions().unwrap().iter().all(|a| a.builtin));
        let action = AiAction {
            id: "custom-1".into(),
            name: "Proofread".into(),
            prompt_template: "Proofread:\n{selection}".into(),
            sort_order: 50,
            builtin: false,
        };
        db.save_ai_action(&action).unwrap();
        let listed = db.list_ai_actions().unwrap();
        assert_eq!(
            listed.len(),
            crate::domain::models::BUILTIN_AI_ACTIONS.len() + 1
        );
        assert!(listed.iter().any(|a| a.id == "custom-1" && !a.builtin));

        db.delete_ai_action("custom-1").unwrap();
        assert!(db.get_ai_action("custom-1").unwrap().is_none());
        assert!(db.delete_ai_action("custom-1").is_err());
        // Missing/blank id and name rejected.
        assert!(db
            .save_ai_action(&AiAction {
                id: "".into(),
                ..action.clone()
            })
            .is_err());
        assert!(db
            .save_ai_action(&AiAction {
                name: "  ".into(),
                ..action
            })
            .is_err());
    }

    #[test]
    fn ai_action_macro_resolves_template_and_substitutes_clipboard() {
        // Loopback server captures the prompt so we assert the full resolution chain:
        // action row → template → {clipboard}/{selection} substitution → AI call.
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
            let _ = stream.write_all(resp.as_bytes());
            req
        });

        let db = db();
        let mut s = Settings::defaults();
        s.ai_enabled = true;
        s.ai_api_key = "k".into();
        s.ai_base_url = format!("http://{addr}");
        db.save_settings(&s).unwrap();

        db.save_ai_action(&AiAction {
            id: "cap".into(),
            name: "Cap".into(),
            prompt_template: "Rewrite this: {clipboard} / {selection}".into(),
            sort_order: 99,
            builtin: false,
        })
        .unwrap();

        let f = new_folder(&db, "F");
        let phrase = new_phrase(
            &db,
            &f.id,
            "AI",
            vec![Segment::Macro {
                macro_value: Macro::AiAction {
                    action_id: "cap".into(),
                },
            }],
        );
        let (expanded, _) = tauri::async_runtime::block_on(db.expand_with_clip(
            &phrase,
            &HashMap::new(),
            Some("CLIP"),
        ))
        .unwrap();
        assert_eq!(expanded, "ok");

        let req = handle.join().unwrap();
        assert!(
            req.contains("Rewrite this: CLIP / CLIP"),
            "prompt not substituted: {req}"
        );
    }

    #[test]
    fn ai_action_macro_with_missing_action_is_not_found() {
        let db = db();
        let f = new_folder(&db, "F");
        let phrase = new_phrase(
            &db,
            &f.id,
            "AI",
            vec![Segment::Macro {
                macro_value: Macro::AiAction {
                    action_id: "gone".into(),
                },
            }],
        );
        let err = tauri::async_runtime::block_on(db.expand(&phrase, &HashMap::new())).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn delay_macro_produces_ordered_chunks_and_flattens() {
        let db = db();
        let f = new_folder(&db, "F");
        let phrase = new_phrase(
            &db,
            &f.id,
            "Delayed",
            vec![
                Segment::Text {
                    text: "before".into(),
                },
                Segment::Macro {
                    macro_value: Macro::Delay { ms: 250 },
                },
                Segment::Text {
                    text: "after".into(),
                },
                // Out-of-range values clamp at expand (no panic, no huge stall).
                Segment::Macro {
                    macro_value: Macro::Delay { ms: 99_999 },
                },
            ],
        );
        let chunks = tauri::async_runtime::block_on(db.expand_chunks_with_clip(
            &phrase,
            &HashMap::new(),
            Some("CLIP"),
        ))
        .unwrap();
        assert_eq!(
            chunks,
            vec![
                OutChunk::Text("before".into()),
                OutChunk::Delay(250),
                OutChunk::Text("after".into()),
                OutChunk::Delay(5000), // clamped
            ]
        );
        // Flat view: text joins, delays drop.
        let (text, images) = OutChunk::flatten(&chunks);
        assert_eq!(text, "beforeafter");
        assert!(images.is_empty());
    }

    #[test]
    fn tags_roundtrip_normalized_and_searchable() {
        let db = db();
        let f = new_folder(&db, "F");
        let mut p = new_phrase(
            &db,
            &f.id,
            "Tagged",
            vec![Segment::Text {
                text: "hello".into(),
            }],
        );
        p.tags = vec![
            "  Sales ".into(),
            "#sales".into(),
            "Invoicing".into(),
            "".into(),
        ];
        db.update_phrase(&p).unwrap();
        let loaded = db.get_active_phrase(&p.id).unwrap();
        // Trimmed + lowercased + `#` stripped + deduped; blanks dropped; order kept.
        assert_eq!(
            loaded.tags,
            vec!["sales".to_string(), "invoicing".to_string()]
        );
        // Summary carries tags for the UI.
        assert_eq!(db.summary(&loaded).tags, loaded.tags);
        // Update replaces the set.
        let mut updated = loaded.clone();
        updated.tags = vec!["Legal".into()];
        db.update_phrase(&updated).unwrap();
        assert_eq!(
            db.get_active_phrase(&p.id).unwrap().tags,
            vec!["legal".to_string()]
        );
    }

    #[test]
    fn phrase_versions_are_capped_per_phrase() {
        let db = db();
        let f = new_folder(&db, "F");
        let mut p = new_phrase(&db, &f.id, "V", vec![Segment::Text { text: "v0".into() }]);
        // 60 edits → 60 snapshots, capped at MAX_PHRASE_VERSIONS, newest kept.
        for i in 0..60 {
            p.body = vec![Segment::Text {
                text: format!("v{i}"),
            }];
            db.update_phrase(&p).unwrap();
        }
        let versions = db.list_versions(&p.id).unwrap();
        assert_eq!(versions.len() as i64, 50);
        // Newest kept. snapshot_current runs BEFORE each update, so 60 edits snapshot
        // bodies v0..v58 (the pre-edit body) — newest is v58, not v59.
        assert!(
            versions[0].preview.contains("v58"),
            "newest first, got {:?}",
            versions[0].preview
        );
    }

    #[test]
    fn cursor_macro_marks_the_caret_position_in_chunks() {
        let db = db();
        let f = new_folder(&db, "F");
        let phrase = new_phrase(
            &db,
            &f.id,
            "Template",
            vec![
                Segment::Text {
                    text: "Hello ".into(),
                },
                Segment::Macro {
                    macro_value: Macro::Cursor,
                },
                Segment::Text {
                    text: "world".into(),
                },
            ],
        );
        let chunks = tauri::async_runtime::block_on(db.expand_chunks_with_clip(
            &phrase,
            &HashMap::new(),
            None,
        ))
        .unwrap();
        assert_eq!(
            chunks,
            vec![
                OutChunk::Text("Hello ".into()),
                OutChunk::Cursor,
                OutChunk::Text("world".into()),
            ]
        );
        // Flat view: full text, cursor invisible (preview/tests).
        let (text, _) = OutChunk::flatten(&chunks);
        assert_eq!(text, "Hello world");
    }
}
