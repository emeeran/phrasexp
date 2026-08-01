//! SQLite repository via rusqlite — mirrors `backend/app.py` `Repository` 1:1.
//! Same schema (migrations/0001_init.sql), same invariants. FR-11 persistence.

use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::{
    ClipboardEntry, ContentType, ExpandRequest, Folder, Phrase, PhraseSummary, Segment, Settings,
    TriggerBinding, TriggerRequest, TriggerType,
};
use crate::domain::{expand_phrase, literal_preview};

pub const CLIPBOARD_CAPACITY: usize = 100; // FR-9 / P3 review O1

const MIGRATION_SQL: &str = include_str!("../../migrations/0001_init.sql");

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn db_err(e: rusqlite::Error) -> DomainError {
    DomainError::Validation(format!("database error: {e}"))
}

fn content_type_str(c: ContentType) -> &'static str {
    match c {
        ContentType::Plain => "plain",
        ContentType::Richtext => "richtext",
        ContentType::Html => "html",
        ContentType::Word => "word",
    }
}

fn parse_content_type(s: &str) -> ContentType {
    match s {
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
    fn assert_abbreviation_unique(&self, phrase: &Phrase) -> DomainResult<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(abbr) = &phrase.abbreviation {
            let clash: bool = conn
                .query_row(
                    "SELECT 1 FROM phrases \
                     WHERE abbreviation = ?1 AND deleted_at IS NULL AND id != ?2 LIMIT 1",
                    params![abbr, phrase.id],
                    |_| Ok(true),
                )
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if clash {
                return Err(DomainError::Conflict(format!(
                    "abbreviation '{abbr}' already in use"
                )));
            }
        }
        Ok(())
    }

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
        self.assert_abbreviation_unique(phrase)?;
        let body_json = serde_json::to_string(&phrase.body)
            .map_err(|e| DomainError::Validation(format!("cannot serialize body: {e}")))?;
        let triggers_json = serde_json::to_string(&phrase.triggers)
            .map_err(|e| DomainError::Validation(format!("cannot serialize triggers: {e}")))?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO phrases \
             (id, folder_id, name, abbreviation, content_type, body_json, triggers_json, \
              sensitive, created_at, updated_at, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
            params![
                phrase.id,
                phrase.folder_id,
                phrase.name,
                phrase.abbreviation,
                content_type_str(phrase.content_type),
                body_json,
                triggers_json,
                phrase.sensitive as i64,
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
                        triggers_json, sensitive, created_at, updated_at, deleted_at \
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
                        triggers_json, sensitive, created_at, updated_at, deleted_at FROM phrases",
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
        self.assert_abbreviation_unique(phrase)?;
        let body_json = serde_json::to_string(&phrase.body)
            .map_err(|e| DomainError::Validation(format!("cannot serialize body: {e}")))?;
        let triggers_json = serde_json::to_string(&phrase.triggers)
            .map_err(|e| DomainError::Validation(format!("cannot serialize triggers: {e}")))?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE phrases SET name=?1, abbreviation=?2, content_type=?3, body_json=?4, \
             triggers_json=?5, sensitive=?6, updated_at=?7 WHERE id=?8",
            params![
                phrase.name,
                phrase.abbreviation,
                content_type_str(phrase.content_type),
                body_json,
                triggers_json,
                phrase.sensitive as i64,
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
    // Settings (M5)
    // ----------------------------------------------------------------------- //
    pub fn load_settings(&self) -> DomainResult<Settings> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT popup_hotkey, autotext_enabled, clipboard_enabled, autostart \
                 FROM settings WHERE id = 1",
                [],
                |r| {
                    Ok(Settings {
                        popup_hotkey: r.get(0)?,
                        autotext_enabled: r.get::<_, i64>(1)? != 0,
                        clipboard_enabled: r.get::<_, i64>(2)? != 0,
                        autostart: r.get::<_, i64>(3)? != 0,
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
                    "INSERT INTO settings (id, popup_hotkey, autotext_enabled, clipboard_enabled, autostart) \
                     VALUES (1, ?1, ?2, ?3, ?4)",
                    params![
                        s.popup_hotkey,
                        s.autotext_enabled as i64,
                        s.clipboard_enabled as i64,
                        s.autostart as i64
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
            "INSERT INTO settings (id, popup_hotkey, autotext_enabled, clipboard_enabled, autostart) \
             VALUES (1, ?1, ?2, ?3, ?4) \
             ON CONFLICT(id) DO UPDATE SET \
                popup_hotkey=excluded.popup_hotkey, \
                autotext_enabled=excluded.autotext_enabled, \
                clipboard_enabled=excluded.clipboard_enabled, \
                autostart=excluded.autostart",
            params![
                s.popup_hotkey,
                s.autotext_enabled as i64,
                s.clipboard_enabled as i64,
                s.autostart as i64
            ],
        )
        .map_err(db_err)?;
        Ok(s.clone())
    }
}

/// Expand a resolved phrase (used by commands/triggers).
pub fn expand(phrase: &Phrase, req: &ExpandRequest) -> DomainResult<String> {
    expand_phrase(phrase, &req.inputs, chrono::Utc::now())
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
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        deleted_at: r.get(10)?,
    })
}

fn get_phrase_inner(conn: &Connection, id: &str) -> DomainResult<Option<Phrase>> {
    let row = conn
        .query_row(
            "SELECT id, folder_id, name, abbreviation, content_type, body_json, \
                    triggers_json, sensitive, created_at, updated_at, deleted_at \
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
    fn abbreviation_conflict_is_rejected() {
        let db = db();
        let f = new_folder(&db, "Inbox");
        let mut a = new_phrase(&db, &f.id, "A", vec![Segment::Text { text: "a".into() }]);
        a.abbreviation = Some("sig".into());
        a.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("sig".into()),
            key: None,
        }];
        db.update_phrase(&a).unwrap();
        let mut b = new_phrase(&db, &f.id, "B", vec![Segment::Text { text: "b".into() }]);
        b.abbreviation = Some("sig".into());
        let err = db.create_phrase(&b).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
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
                    macro_value: Macro::InputForm { field: "n".into() },
                },
            ],
        );
        p.abbreviation = Some("hi".into());
        p.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some("hi".into()),
            key: None,
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
        let req = ExpandRequest {
            trigger: None,
            inputs,
        };
        assert_eq!(expand(&phrase, &req).unwrap(), "Hello Sam");
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
