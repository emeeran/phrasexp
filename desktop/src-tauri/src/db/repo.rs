//! SQLite repository via rusqlite — mirrors `backend/app.py` `Repository` 1:1.
//! Same schema (migrations/0001_init.sql), same invariants. FR-11 persistence.

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use std::collections::HashMap;

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::{
    ClipboardEntry, ContentType, Folder, Macro, Phrase, PhraseSummary, PhraseVersion, Segment,
    Settings, TriggerBinding, TriggerRequest, TriggerType,
};
use crate::domain::{evaluate_macro, literal_preview};

pub const CLIPBOARD_CAPACITY: usize = 100; // FR-9 / P3 review O1

const MIGRATION_SQL: &str = include_str!("../../migrations/0001_init.sql");

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn db_err(e: rusqlite::Error) -> DomainError {
    DomainError::Validation(format!("database error: {e}"))
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

/// Literal-text preview of a segment list (for version history list).
fn segments_preview(segments: &[Segment]) -> String {
    let mut text = String::new();
    for seg in segments {
        if let Segment::Text { text: t } = seg {
            text.push_str(t);
        }
    }
    if text.chars().count() > 60 {
        let t: String = text.chars().take(60).collect();
        format!("{}…", t.trim_end())
    } else {
        text
    }
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

fn parse_content_type(s: &str) -> ContentType {
    match s {
        "markdown" => ContentType::Markdown,
        "richtext" => ContentType::Richtext,
        "html" => ContentType::Html,
        "word" => ContentType::Word,
        _ => ContentType::Plain,
    }
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> DomainResult<Self> {
        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    pub fn open_in_memory() -> DomainResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
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
            "ALTER TABLE settings ADD COLUMN ai_base_url TEXT NOT NULL DEFAULT 'https://api.openai.com/v1'",
            "ALTER TABLE settings ADD COLUMN ai_api_key TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE settings ADD COLUMN ai_model TEXT NOT NULL DEFAULT 'gpt-4o-mini'",
            "ALTER TABLE phrases ADD COLUMN app_scope TEXT",
        ] {
            let _ = conn.execute(stmt, []);
        }
        Ok(())
    }

    // ----------------------------------------------------------------------- //
    // Folders (FR-2)
    // ----------------------------------------------------------------------- //
    pub fn create_folder(&self, folder: &Folder) -> DomainResult<Folder> {
        let conn = self.conn.lock().unwrap();
        if let Some(parent) = &folder.parent_id {
            let exists: bool = conn
                .query_row("SELECT 1 FROM folders WHERE id = ?1", params![parent], |_| Ok(true))
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if !exists {
                return Err(DomainError::NotFound(format!("parent folder {parent} not found")));
            }
            guard_no_cycle(&conn, &folder.id, parent)?;
        }
        conn.execute(
            "INSERT INTO folders (id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![folder.id, folder.name, folder.parent_id, folder.created_at],
        )
        .map_err(db_err)?;
        Ok(folder.clone())
    }

    pub fn get_folder(&self, id: &str) -> DomainResult<Option<Folder>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, name, parent_id, created_at FROM folders WHERE id = ?1",
                params![id],
                row_to_folder,
            )
            .optional()
            .map_err(db_err)?;
        Ok(row)
    }

    pub fn list_folders(&self) -> DomainResult<Vec<Folder>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, parent_id, created_at FROM folders")
            .map_err(db_err)?;
        let rows = stmt.query_map([], row_to_folder).map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub fn delete_folder(&self, id: &str) -> DomainResult<bool> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT 1 FROM folders WHERE id = ?1", params![id], |_| Ok(true))
            .optional()
            .map_err(db_err)?
            .unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        // P3 review O3: refuse if active (non-deleted) phrases reference it.
        let has_active: bool = conn
            .query_row(
                "SELECT 1 FROM phrases WHERE folder_id = ?1 AND deleted_at IS NULL LIMIT 1",
                params![id],
                |_| Ok(true),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(false);
        if has_active {
            return Err(DomainError::FolderNotEmpty(format!(
                "folder {id} still contains active phrases"
            )));
        }
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(true)
    }

    // ----------------------------------------------------------------------- //
    // Phrases (FR-1, FR-6, FR-7, FR-8, FR-10)
    // ----------------------------------------------------------------------- //
    // Abbreviation uniqueness is intentionally NOT enforced — multiple phrases may share
    // an abbreviation; a multi-match selection menu resolves ambiguity (Phase 1).

    pub fn create_phrase(&self, phrase: &Phrase) -> DomainResult<Phrase> {
        phrase.validate()?;
        {
            let conn = self.conn.lock().unwrap();
            let folder_exists: bool = conn
                .query_row(
                    "SELECT 1 FROM folders WHERE id = ?1",
                    params![phrase.folder_id],
                    |_| Ok(true),
                )
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if !folder_exists {
                return Err(DomainError::NotFound(format!(
                    "folder {} not found",
                    phrase.folder_id
                )));
            }
        }
        let body_json = serde_json::to_string(&phrase.body)
            .map_err(|e| DomainError::Validation(format!("cannot serialize body: {e}")))?;
        let triggers_json = serde_json::to_string(&phrase.triggers)
            .map_err(|e| DomainError::Validation(format!("cannot serialize triggers: {e}")))?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO phrases \
             (id, folder_id, name, abbreviation, content_type, body_json, triggers_json, \
              sensitive, app_scope, created_at, updated_at, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
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
                phrase.created_at,
                phrase.updated_at,
            ],
        )
        .map_err(db_err)?;
        Ok(phrase.clone())
    }

    pub fn get_phrase(&self, id: &str) -> DomainResult<Option<Phrase>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                        triggers_json, sensitive, app_scope, created_at, updated_at, deleted_at \
                 FROM phrases WHERE id = ?1",
                params![id],
                row_to_phrase,
            )
            .optional()
            .map_err(db_err)?;
        Ok(row)
    }

    /// Filters applied in Rust (library sizes are modest; avoids dynamic SQL plumbing).
    pub fn list_phrases(
        &self,
        folder_id: Option<&str>,
        q: Option<&str>,
        include_deleted: bool,
    ) -> DomainResult<Vec<Phrase>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                        triggers_json, sensitive, app_scope, created_at, updated_at, deleted_at FROM phrases",
            )
            .map_err(db_err)?;
        let rows = stmt.query_map([], row_to_phrase).map_err(db_err)?;
        let needle = q.map(|s| s.to_lowercase());
        let mut out = Vec::new();
        for row in rows {
            let p = row.map_err(db_err)?;
            if !include_deleted && p.deleted_at.is_some() {
                continue;
            }
            if let Some(fid) = folder_id {
                if p.folder_id != fid {
                    continue;
                }
            }
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

    pub fn update_phrase(&self, phrase: &Phrase) -> DomainResult<Phrase> {
        phrase.validate()?;
        {
            let conn = self.conn.lock().unwrap();
            let exists: bool = conn
                .query_row("SELECT 1 FROM phrases WHERE id = ?1", params![phrase.id], |_| {
                    Ok(true)
                })
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if !exists {
                return Err(DomainError::NotFound(format!("phrase {} not found", phrase.id)));
            }
        }
        self.snapshot_current(&phrase.id)?;
        let body_json = serde_json::to_string(&phrase.body)
            .map_err(|e| DomainError::Validation(format!("cannot serialize body: {e}")))?;
        let triggers_json = serde_json::to_string(&phrase.triggers)
            .map_err(|e| DomainError::Validation(format!("cannot serialize triggers: {e}")))?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE phrases SET name=?1, abbreviation=?2, content_type=?3, body_json=?4, \
             triggers_json=?5, sensitive=?6, app_scope=?7, updated_at=?8 WHERE id=?9",
            params![
                phrase.name,
                phrase.abbreviation,
                content_type_str(phrase.content_type),
                body_json,
                triggers_json,
                phrase.sensitive as i64,
                phrase.app_scope,
                now_iso(),
                phrase.id,
            ],
        )
        .map_err(db_err)?;
        Ok(phrase.clone())
    }

    pub fn soft_delete_phrase(&self, id: &str) -> DomainResult<Option<Phrase>> {
        let conn = self.conn.lock().unwrap();
        let existing = get_phrase_inner(&conn, id)?;
        match existing {
            None => Ok(None),
            Some(p) if p.deleted_at.is_some() => Ok(None),
            Some(_) => {
                conn.execute(
                    "UPDATE phrases SET deleted_at = ?1 WHERE id = ?2",
                    params![now_iso(), id],
                )
                .map_err(db_err)?;
                get_phrase_inner(&conn, id)
            }
        }
    }

    /// FR-4 resolution. Deleted phrases never resolve (FR-7).
    /// P3 review O2: first match in storage order wins (hotkey uniqueness not enforced in v1).
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
    // Clipboard (FR-9)
    // ----------------------------------------------------------------------- //
    pub fn add_clipboard(&self, entry: &ClipboardEntry) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_entries (id, text, captured_at) VALUES (?1, ?2, ?3)",
            params![entry.id, entry.text, entry.captured_at],
        )
        .map_err(db_err)?;
        // FR-9 / P3 O1: capacity-bounded — evict oldest on overflow.
        let ids: Vec<String> = conn
            .prepare("SELECT id FROM clipboard_entries ORDER BY captured_at DESC")
            .map_err(db_err)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        if ids.len() > CLIPBOARD_CAPACITY {
            for stale in ids.iter().skip(CLIPBOARD_CAPACITY) {
                conn.execute("DELETE FROM clipboard_entries WHERE id = ?1", params![stale])
                    .map_err(db_err)?;
            }
        }
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
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    /// Build a masked summary for list views (FR-8).
    pub fn summary(&self, phrase: &Phrase) -> PhraseSummary {
        let preview = if phrase.sensitive {
            "***".to_string()
        } else {
            literal_preview(phrase)
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
        }
    }

    // ----------------------------------------------------------------------- //
    // Expansion (Clipboard macro needs the system clipboard)
    // ----------------------------------------------------------------------- //
    pub fn expand(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
    ) -> DomainResult<String> {
        let now = chrono::Utc::now();
        let clip = read_clipboard_text();
        self.expand_inner(phrase, inputs, now, clip.as_deref(), 0)
    }

    /// Recursive walk: handles Nested (linked phrases, depth-limited) and Clipboard;
    /// delegates the pure macros to evaluate_macro.
    fn expand_inner(
        &self,
        phrase: &Phrase,
        inputs: &HashMap<String, String>,
        now: chrono::DateTime<chrono::Utc>,
        clip: Option<&str>,
        depth: u8,
    ) -> DomainResult<String> {
        if depth > 8 {
            return Err(DomainError::Validation("phrase nesting too deep".into()));
        }
        let mut out = String::new();
        for seg in &phrase.body {
            match seg {
                Segment::Text { text } => out.push_str(text),
                Segment::Macro { macro_value } => match macro_value {
                    Macro::Clipboard => out.push_str(clip.unwrap_or("")),
                    Macro::Ai { prompt } => {
                        let settings = self.load_settings()?;
                        out.push_str(&crate::triggers::ai::complete(&settings, prompt)?);
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
                        out.push_str(&self.expand_inner(&linked, inputs, now, clip, depth + 1)?);
                    }
                    other => out.push_str(&evaluate_macro(other, inputs, now)?),
                },
            }
        }
        Ok(out)
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
    // Version history (Phase 1)
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
        }
        Ok(())
    }

    pub fn list_versions(&self, phrase_id: &str) -> DomainResult<Vec<PhraseVersion>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, phrase_id, body_json, triggers_json, saved_at FROM phrase_versions \
                 WHERE phrase_id = ?1 ORDER BY saved_at DESC",
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
            let body: Vec<Segment> = serde_json::from_str(&body_json).unwrap_or_default();
            let triggers: Vec<TriggerBinding> =
                serde_json::from_str(&triggers_json).unwrap_or_default();
            out.push(PhraseVersion {
                id,
                phrase_id: pid,
                preview: segments_preview(&body),
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
            let (bj, tj) = row
                .ok_or_else(|| DomainError::NotFound("version not found".into()))?;
            let body: Vec<Segment> = serde_json::from_str(&bj)
                .map_err(|e| DomainError::Validation(format!("version body: {e}")))?;
            let triggers: Vec<TriggerBinding> = serde_json::from_str(&tj).unwrap_or_default();
            (body, triggers)
        };
        phrase.body = body;
        phrase.triggers = triggers;
        self.update_phrase(&phrase) // snapshots current (pre-restore) + applies
    }

    // ----------------------------------------------------------------------- //
    // Settings (M5)
    // ----------------------------------------------------------------------- //
    pub fn load_settings(&self) -> DomainResult<Settings> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT popup_hotkey, autotext_enabled, clipboard_enabled, autostart, \
                        ai_enabled, ai_base_url, ai_api_key, ai_model \
                 FROM settings WHERE id = 1",
                [],
                |r| {
                    Ok(Settings {
                        popup_hotkey: r.get(0)?,
                        autotext_enabled: r.get::<_, i64>(1)? != 0,
                        clipboard_enabled: r.get::<_, i64>(2)? != 0,
                        autostart: r.get::<_, i64>(3)? != 0,
                        ai_enabled: r.get::<_, i64>(4)? != 0,
                        ai_base_url: r.get(5)?,
                        ai_api_key: r.get(6)?,
                        ai_model: r.get(7)?,
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
                     autostart, ai_enabled, ai_base_url, ai_api_key, ai_model) \
                     VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        s.popup_hotkey,
                        s.autotext_enabled as i64,
                        s.clipboard_enabled as i64,
                        s.autostart as i64,
                        s.ai_enabled as i64,
                        s.ai_base_url,
                        s.ai_api_key,
                        s.ai_model
                    ],
                )
                .map_err(db_err)?;
                Ok(s)
            }
        }
    }

    pub fn save_settings(&self, s: &Settings) -> DomainResult<Settings> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (id, popup_hotkey, autotext_enabled, clipboard_enabled, \
             autostart, ai_enabled, ai_base_url, ai_api_key, ai_model) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(id) DO UPDATE SET \
                popup_hotkey=excluded.popup_hotkey, \
                autotext_enabled=excluded.autotext_enabled, \
                clipboard_enabled=excluded.clipboard_enabled, \
                autostart=excluded.autostart, \
                ai_enabled=excluded.ai_enabled, \
                ai_base_url=excluded.ai_base_url, \
                ai_api_key=excluded.ai_api_key, \
                ai_model=excluded.ai_model",
            params![
                s.popup_hotkey,
                s.autotext_enabled as i64,
                s.clipboard_enabled as i64,
                s.autostart as i64,
                s.ai_enabled as i64,
                s.ai_base_url,
                s.ai_api_key,
                s.ai_model
            ],
        )
        .map_err(db_err)?;
        Ok(s.clone())
    }
}

fn row_to_folder(r: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        name: r.get(1)?,
        parent_id: r.get(2)?,
        created_at: r.get(3)?,
    })
}

fn row_to_phrase(r: &rusqlite::Row) -> rusqlite::Result<Phrase> {
    let body_json: String = r.get(5)?;
    let triggers_json: String = r.get(6)?;
    let content_type: String = r.get(4)?;
    let body: Vec<Segment> = serde_json::from_str(&body_json).unwrap_or_default();
    let triggers: Vec<TriggerBinding> = serde_json::from_str(&triggers_json).unwrap_or_default();
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
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        deleted_at: r.get(11)?,
    })
}

fn get_phrase_inner(conn: &Connection, id: &str) -> DomainResult<Option<Phrase>> {
    let row = conn
        .query_row(
            "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                    triggers_json, sensitive, app_scope, created_at, updated_at, deleted_at \
             FROM phrases WHERE id = ?1",
            params![id],
            row_to_phrase,
        )
        .optional()
        .map_err(db_err)?;
    Ok(row)
}

/// FR-2. Walk up the parent chain; reaching ourselves means a cycle.
/// (Defensive — unreachable via the create-only API, but mirrors the reference impl.)
fn guard_no_cycle(conn: &Connection, folder_id: &str, parent_id: &str) -> DomainResult<()> {
    let mut cursor = parent_id.to_string();
    let mut seen = std::collections::HashSet::new();
    loop {
        if cursor.as_str() == folder_id || seen.contains(&cursor) {
            return Err(DomainError::Cycle("folder parent_id would create a cycle".into()));
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
    use crate::domain::models::{Macro, PhraseUpdate};
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_folder(db: &Db, name: &str) -> Folder {
        let f = Folder {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            parent_id: None,
            created_at: now_iso(),
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
            created_at: now_iso(),
            updated_at: now_iso(),
            deleted_at: None,
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
        // Phase 1: multiple phrases may share an abbreviation (resolved by a selection menu).
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut a = new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "a".into() }]);
        a.abbreviation = Some("sig".into());
        a.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("sig".into()),
            key: None,
            regex: false,
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
    fn folder_not_empty_blocks_delete() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "x".into() }]);
        let err = db.delete_folder(&f.id).unwrap_err();
        assert!(matches!(err, DomainError::FolderNotEmpty(_)));
    }

    #[test]
    fn clipboard_capacity_evicts_oldest() {
        let db = db();
        for i in 0..(CLIPBOARD_CAPACITY as u32 + 1) {
            db.add_clipboard(&clip(&format!("item-{i}"), i)).unwrap();
        }
        let listed = db.list_clipboard((CLIPBOARD_CAPACITY as i64) + 1).unwrap();
        assert_eq!(listed.len(), CLIPBOARD_CAPACITY);
        assert!(listed.iter().all(|e| e.text != "item-0"));
        assert!(listed
            .iter()
            .any(|e| e.text == format!("item-{CLIPBOARD_CAPACITY}")));
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
                Segment::Text { text: "Hello ".into() },
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
        assert_eq!(db.expand(&phrase, &inputs).unwrap(), "Hello Sam");
    }

    #[test]
    fn nested_phrase_expands_recursively() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let inner = new_phrase(&db, &f.id, "Inner", vec![Segment::Text { text: "World".into() }]);
        let outer = new_phrase(
            &db,
            &f.id,
            "Outer",
            vec![
                Segment::Text { text: "Hello ".into() },
                Segment::Macro {
                    macro_value: Macro::Nested { phrase_id: inner.id.clone() },
                },
            ],
        );
        assert_eq!(db.expand(&outer, &HashMap::new()).unwrap(), "Hello World");
    }

    #[test]
    fn find_duplicates_groups_identical_content() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "same text".into() }]);
        new_phrase(&db, &f.id, "B", vec![Segment::Text { text: "same text".into() }]);
        new_phrase(&db, &f.id, "C", vec![Segment::Text { text: "unique".into() }]);
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

    // keep PhraseUpdate referenced
    #[test]
    fn dto_types_present() {
        let _u: PhraseUpdate = PhraseUpdate::default();
    }
}
