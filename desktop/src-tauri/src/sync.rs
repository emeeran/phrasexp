//! File-based library sync — TextExpander-style multi-device, but local-first: the
//! "server" is a folder the user owns (Syncthing, WebDAV mount, git repo, USB stick).
//! One JSON snapshot per device-independent location holds the merged library;
//! merging is per-phrase last-writer-wins on `updated_at`, with soft-deletes
//! propagating as tombstones. Settings, counters, and clipboard history are
//! deliberately local-only.
//!
//! Convergence contract: after `sync_now`, the local DB and the file hold the same
//! phrase set, and applying a snapshot never bumps timestamps (`upsert_synced_phrase`)
//! — so two devices syncing in rotation stabilize instead of ping-ponging copies.

use crate::db::Db;
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::{Folder, Phrase};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Name of the snapshot file inside the sync folder.
pub const SYNC_FILE: &str = "phrasexp-library.json";

/// What one sync run did — surfaced in Settings → Data and unit-test assertions.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SyncReport {
    /// Phrases that existed only in the file → created here.
    pub imported: usize,
    /// Remote copy newer than local → overwrote local (previous row snapshotted).
    pub updated: usize,
    /// Remote tombstone newer than local → soft-deleted here.
    pub deleted: usize,
    /// Phrases in the merged snapshot written back to the file.
    pub stored: usize,
    /// Remote phrases rejected while applying (untrusted macros, invalid rows).
    pub skipped: usize,
}

/// The on-disk snapshot. Tombstones (deleted_at set) ride along so deletions
/// propagate; they are ordinary phrases to every reader.
#[derive(Debug, Serialize, Deserialize)]
struct SyncSnapshot {
    device: String,
    exported_at: String,
    folders: Vec<Folder>,
    phrases: Vec<Phrase>,
}

/// Merge remote against local, last-writer-wins per phrase on `updated_at`
/// (ISO-8601 strings compare correctly as plain text). Returns the phrases that
/// must be applied to the local DB, the merged phrase set that belongs in the
/// file, and the counts. Pure — unit-tested without a DB.
fn merge_lww(remote: &[Phrase], local: &[Phrase]) -> (Vec<Phrase>, Vec<Phrase>, SyncReport) {
    let mut report = SyncReport::default();
    let local_by_id: std::collections::HashMap<&str, &Phrase> =
        local.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut to_apply = Vec::new();
    let mut merged: Vec<Phrase> = local.to_vec();
    for r in remote {
        match local_by_id.get(r.id.as_str()) {
            None => {
                // Only in the file → create here, and it belongs in the merge.
                report.imported += 1;
                to_apply.push(r.clone());
                merged.push(r.clone());
            }
            Some(l) => {
                if r.updated_at.as_str() > l.updated_at.as_str() {
                    if r.deleted_at.is_some() && l.deleted_at.is_none() {
                        report.deleted += 1;
                    } else if l.deleted_at.is_none() {
                        report.updated += 1;
                    }
                    to_apply.push(r.clone());
                    // Replace the local entry with the newer remote copy.
                    if let Some(slot) = merged.iter_mut().find(|p| p.id == r.id) {
                        *slot = r.clone();
                    }
                }
            }
        }
    }
    (to_apply, merged, report)
}

/// True when the phrase's folder exists among `folders` — phrases referencing a
/// folder neither side knows would violate the FK on insert.
fn folder_known(folders: &[Folder], folder_id: &str) -> bool {
    folders.iter().any(|f| f.id == folder_id)
}

/// Best-effort device label for the snapshot header (informational only).
fn device_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

fn snapshot_timestamp() -> String {
    // Same clock style the repo layer stamps — UTC RFC-3339 via chrono.
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Serializes sync runs (auto-sync threads + manual "Sync now" + startup sync can
/// otherwise interleave read-modify-write of the snapshot file).
static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Pull + push once. Errors mean "sync didn't happen" — the file and DB are left
/// untouched (snapshot writes are temp-file + rename).
pub fn sync_now(db: &Db) -> DomainResult<SyncReport> {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let settings = db.load_settings()?;
    let dir = PathBuf::from(settings.sync_dir.trim());
    if dir.as_os_str().is_empty() {
        return Err(DomainError::Validation(
            "No sync folder configured — set one in Settings → Data".into(),
        ));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| DomainError::Validation(format!("sync folder: {e}")))?;
    let path = dir.join(SYNC_FILE);

    // "No snapshot yet" (first sync on a new device) and "snapshot exists" are the
    // only states allowed through to merge — and merge result is pushed back over
    // the file. A corrupt or unreadable snapshot must NOT look empty: the push would
    // replace the other devices' library with ours. Refuse and leave the file alone.
    let remote = match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<SyncSnapshot>(&s) {
            Ok(s) => Some(s),
            Err(e) => {
                return Err(DomainError::Validation(format!(
                    "sync snapshot is corrupt — not syncing (fix or delete {SYNC_FILE}): {e}"
                )));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(DomainError::Validation(format!(
                "sync snapshot unreadable — not syncing: {e}"
            )));
        }
    };
    let (remote_folders, remote_phrases) = match remote {
        Some(s) => (s.folders, s.phrases),
        None => (Vec::new(), Vec::new()),
    };
    // Trust boundary: refuse remote phrases carrying Shell/File/Env macros before
    // they enter the merge — they must not be applied here, and must not ride
    // along in the snapshot we write back (a hostile writer can rewrite the file,
    // but our push should not propagate the payload for other devices to skip).
    let skipped_untrusted = remote_phrases.iter().filter(|p| p.has_untrusted_macros()).count();
    if skipped_untrusted > 0 {
        crate::debug_log(&format!(
            "sync: refusing {skipped_untrusted} phrase(s) with Shell/File/Env macros"
        ));
    }
    let remote_phrases: Vec<Phrase> = remote_phrases
        .into_iter()
        .filter(|p| !p.has_untrusted_macros())
        .collect();
    let local_folders = db.list_folders()?;
    let local_phrases = db.list_phrases(None, None, true)?; // include tombstones

    let (to_apply, merged_phrases, mut report) = merge_lww(&remote_phrases, &local_phrases);
    report.skipped = skipped_untrusted;

    // Folders the file knows that we don't — apply before phrases (FK), and backfill
    // a placeholder for any applied phrase whose folder is unknown to BOTH sides
    // (e.g. the remote device pruned the folder without tombstone support).
    let mut all_folders = local_folders.clone();
    for f in &remote_folders {
        if !folder_known(&all_folders, &f.id) {
            db.upsert_synced_folder(f)?;
            all_folders.push(f.clone());
        }
    }
    for p in &to_apply {
        if !folder_known(&all_folders, &p.folder_id) {
            let orphan = Folder {
                id: p.folder_id.clone(),
                name: "Synced".into(),
                parent_id: None,
                created_at: snapshot_timestamp(),
                enabled: true,
            };
            db.upsert_synced_folder(&orphan)?;
            all_folders.push(orphan);
        }
    }
    for p in &to_apply {
        // One invalid phrase must not abort the run mid-apply (sync would then
        // brake permanently on this snapshot); skip it and surface the count.
        if let Err(e) = db.upsert_synced_phrase(p) {
            crate::debug_log(&format!("sync: skipping phrase {}: {e}", p.id));
            report.skipped += 1;
        }
    }

    // Write the merged snapshot back — the push half of pull+push. Temp file +
    // rename so a crash mid-write can't corrupt the file other devices read.
    let snapshot = SyncSnapshot {
        device: device_name(),
        exported_at: snapshot_timestamp(),
        folders: all_folders,
        phrases: merged_phrases,
    };
    report.stored = snapshot.phrases.len();
    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| DomainError::Validation(format!("snapshot serialize: {e}")))?;
    let tmp = dir.join(format!("{SYNC_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| DomainError::Validation(format!("sync write: {e}")))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| DomainError::Validation(format!("sync rename: {e}")))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(id: &str, updated_at: &str, deleted: bool) -> Phrase {
        Phrase {
            id: id.into(),
            folder_id: "f1".into(),
            name: id.into(),
            abbreviation: Some(id.into()),
            content_type: Default::default(),
            body: vec![],
            triggers: vec![],
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            tags: vec![],
            case_mode: Default::default(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: updated_at.into(),
            deleted_at: deleted.then(|| updated_at.into()),
        }
    }

    #[test]
    fn remote_newer_wins_and_counts() {
        let local = vec![phrase("a", "2026-01-01T00:00:00Z", false)];
        let remote = vec![phrase("a", "2026-01-02T00:00:00Z", false)];
        let (apply, merged, report) = merge_lww(&remote, &local);
        assert_eq!(apply.len(), 1);
        assert_eq!(apply[0].updated_at, "2026-01-02T00:00:00Z");
        assert_eq!(merged.len(), 1);
        assert_eq!(report.updated, 1);
        assert_eq!(report.imported, 0);
    }

    #[test]
    fn local_newer_is_untouched() {
        let local = vec![phrase("a", "2026-01-05T00:00:00Z", false)];
        let remote = vec![phrase("a", "2026-01-02T00:00:00Z", false)];
        let (apply, merged, report) = merge_lww(&remote, &local);
        assert!(apply.is_empty());
        assert_eq!(merged[0].updated_at, "2026-01-05T00:00:00Z");
        assert_eq!(report, SyncReport::default());
    }

    #[test]
    fn remote_only_phrase_is_imported() {
        let local = vec![];
        let remote = vec![phrase("new", "2026-01-02T00:00:00Z", false)];
        let (apply, merged, report) = merge_lww(&remote, &local);
        assert_eq!(apply.len(), 1);
        assert_eq!(merged.len(), 1);
        assert_eq!(report.imported, 1);
    }

    #[test]
    fn remote_tombstone_deletes_local() {
        let local = vec![phrase("a", "2026-01-01T00:00:00Z", false)];
        let remote = vec![phrase("a", "2026-01-03T00:00:00Z", true)];
        let (apply, _, report) = merge_lww(&remote, &local);
        assert_eq!(apply.len(), 1);
        assert!(apply[0].deleted_at.is_some());
        assert_eq!(report.deleted, 1);
        assert_eq!(report.updated, 0);
    }

    #[test]
    fn local_delete_survives_older_remote_edit() {
        // Edited here, deleted there, local tombstone newer → keep ours.
        let local = vec![phrase("a", "2026-01-06T00:00:00Z", true)];
        let remote = vec![phrase("a", "2026-01-04T00:00:00Z", false)];
        let (apply, merged, _) = merge_lww(&remote, &local);
        assert!(apply.is_empty());
        assert!(merged[0].deleted_at.is_some());
    }

    #[test]
    fn folder_known_matches_by_id() {
        let folders = vec![Folder {
            id: "f1".into(),
            name: "Work".into(),
            parent_id: None,
            created_at: String::new(),
            enabled: true,
        }];
        assert!(folder_known(&folders, "f1"));
        assert!(!folder_known(&folders, "f2"));
    }

    #[test]
    fn corrupt_snapshot_refuses_sync_and_preserves_file() {
        // A truncated snapshot (crash mid-write, partial cloud download) must abort
        // the run instead of merging against an "empty remote" — the push would then
        // replace the other devices' library with our local-only view.
        let dir =
            std::env::temp_dir().join(format!("phrasexp-sync-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SYNC_FILE);
        let corrupt = r#"{"device": "other", "phrases": [{"id":"#;
        std::fs::write(&path, corrupt).unwrap();

        let db = crate::db::Db::open_in_memory().unwrap();
        let mut settings = db.load_settings().unwrap();
        settings.sync_dir = dir.to_string_lossy().into_owned();
        db.save_settings(&settings).unwrap();

        let err = sync_now(&db).unwrap_err();
        assert!(err.to_string().contains("corrupt"), "got: {err}");
        // The file on disk is byte-identical — nothing was pushed over it.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
        let _ = std::fs::remove_dir_all(&dir);
    }

#[test]
fn untrusted_detection_survives_json_roundtrip() {
    use crate::domain::models::{ContentType, Macro, Segment};
    let evil = Phrase {
        id: "evil".into(),
        folder_id: "f".into(),
        name: "n".into(),
        abbreviation: None,
        content_type: ContentType::Plain,
        body: vec![Segment::Macro { macro_value: Macro::Shell { command: "x".into() } }],
        triggers: Vec::new(),
        sensitive: false,
        app_scope: None,
        expanded_count: 0,
        last_used_at: None,
        case_mode: Default::default(),
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
        tags: Vec::new(),
    };
    assert!(evil.has_untrusted_macros());
    let json = serde_json::to_string(&evil).unwrap();
    eprintln!("JSON: {json}");
    let back: Phrase = serde_json::from_str(&json).unwrap();
    assert!(back.has_untrusted_macros(), "detection lost after roundtrip");
}

    #[test]
    fn missing_snapshot_file_is_first_sync_not_corruption() {
        let dir = std::env::temp_dir().join(format!("phrasexp-sync-fresh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let db = crate::db::Db::open_in_memory().unwrap();
        let mut settings = db.load_settings().unwrap();
        settings.sync_dir = dir.to_string_lossy().into_owned();
        db.save_settings(&settings).unwrap();

        let report = sync_now(&db).unwrap();
        assert_eq!(report.imported, 0);
        assert_eq!(report.stored, 0);
        assert!(dir.join(SYNC_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_snapshot_shell_macro_is_never_applied() {
        use crate::domain::models::{Macro, Segment};
        let dir = std::env::temp_dir().join(format!("phrasexp-sync-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut plain = phrase("ok", "2026-01-02T00:00:00Z", false);
        plain.body = vec![Segment::Text { text: "hello".into() }];
        let mut evil = phrase("evil", "2026-01-02T00:00:00Z", false);
        evil.body = vec![Segment::Macro {
            macro_value: Macro::Shell {
                command: "rm -rf /".into(),
            },
        }];
        let snapshot = SyncSnapshot {
            device: "other".into(),
            exported_at: "2026-01-02T00:00:00Z".into(),
            folders: vec![],
            phrases: vec![plain, evil.clone()],
        };
        std::fs::write(
            dir.join(SYNC_FILE),
            serde_json::to_string(&snapshot).unwrap(),
        )
        .unwrap();

        let db = crate::db::Db::open_in_memory().unwrap();
        let mut settings = db.load_settings().unwrap();
        settings.sync_dir = dir.to_string_lossy().into_owned();
        db.save_settings(&settings).unwrap();

        let report = sync_now(&db).unwrap();
        assert_eq!(report.imported, 1, "only the plain phrase is applied");
        assert_eq!(report.skipped, 1);
        // The Shell phrase must not exist locally in any form.
        assert!(db.get_phrase(&evil.id).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
