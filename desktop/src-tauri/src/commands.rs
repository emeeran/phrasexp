//! Tauri commands — the frontend→Rust API surface. Mirrors the FastAPI endpoints
//! (SPEC.md §6) but over Tauri IPC (no HTTP). Each returns `Result<T, ApiError>`;
//! `ApiError` serializes to `{ code, message }` which the frontend inspects.

use std::sync::atomic::Ordering;

use tauri::State;
use uuid::Uuid;

use crate::domain::models::*;
use crate::domain::{ApiError, DomainError, DomainResult};
use crate::AppState;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn not_found(msg: impl Into<String>) -> ApiError {
    ApiError::from(DomainError::NotFound(msg.into()))
}

// -- Folders (FR-2) -------------------------------------------------------- //
#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, ApiError> {
    Ok(state.db.list_folders()?)
}

#[tauri::command]
pub fn create_folder(payload: FolderCreate, state: State<'_, AppState>) -> Result<Folder, ApiError> {
    let folder = Folder {
        id: Uuid::new_v4().to_string(),
        name: payload.name,
        parent_id: payload.parent_id,
        created_at: now_iso(),
    };
    Ok(state.db.create_folder(&folder)?)
}

#[tauri::command]
pub fn get_folder(folder_id: String, state: State<'_, AppState>) -> Result<Folder, ApiError> {
    state
        .db
        .get_folder(&folder_id)?
        .ok_or_else(|| not_found(format!("folder {folder_id} not found")))
}

#[tauri::command]
pub fn delete_folder(folder_id: String, state: State<'_, AppState>) -> Result<(), ApiError> {
    if state.db.delete_folder(&folder_id)? {
        Ok(())
    } else {
        Err(not_found(format!("folder {folder_id} not found")))
    }
}

// -- Phrases (FR-1, FR-6, FR-7, FR-8, FR-10) ------------------------------- //
#[tauri::command]
pub fn list_phrases(
    folder_id: Option<String>,
    q: Option<String>,
    include_deleted: Option<bool>,
    sort: Option<SortKey>,
    state: State<'_, AppState>,
) -> Result<Vec<PhraseSummary>, ApiError> {
    let mut phrases =
        state
            .db
            .list_phrases(folder_id.as_deref(), q.as_deref(), include_deleted.unwrap_or(false))?;
    match sort.unwrap_or_default() {
        SortKey::Name => phrases.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        SortKey::Updated => phrases.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        SortKey::Created => phrases.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
    }
    Ok(phrases.iter().map(|p| state.db.summary(p)).collect())
}

#[tauri::command]
pub fn get_phrase(phrase_id: String, state: State<'_, AppState>) -> Result<Phrase, ApiError> {
    let p = state
        .db
        .get_phrase(&phrase_id)?
        .ok_or_else(|| not_found(format!("phrase {phrase_id} not found")))?;
    if p.deleted_at.is_some() {
        return Err(not_found(format!("phrase {phrase_id} not found")));
    }
    Ok(p)
}

#[tauri::command]
pub fn create_phrase(
    mut payload: PhraseCreate,
    state: State<'_, AppState>,
) -> Result<Phrase, ApiError> {
    payload.normalize()?;
    let phrase = Phrase {
        id: Uuid::new_v4().to_string(),
        folder_id: payload.folder_id,
        name: payload.name,
        abbreviation: payload.abbreviation,
        content_type: payload.content_type,
        body: payload.body,
        triggers: payload.triggers,
        sensitive: payload.sensitive,
        created_at: now_iso(),
        updated_at: now_iso(),
        deleted_at: None,
    };
    let saved = state.db.create_phrase(&phrase)?;
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    Ok(saved)
}

#[tauri::command]
pub fn update_phrase(
    phrase_id: String,
    payload: PhraseUpdate,
    state: State<'_, AppState>,
) -> Result<Phrase, ApiError> {
    let existing = state
        .db
        .get_phrase(&phrase_id)?
        .ok_or_else(|| not_found(format!("phrase {phrase_id} not found")))?;
    if existing.deleted_at.is_some() {
        return Err(not_found(format!("phrase {phrase_id} not found")));
    }
    let updated = apply_update(existing, payload)?;
    let saved = state.db.update_phrase(&updated)?;
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    Ok(saved)
}

#[tauri::command]
pub fn delete_phrase(phrase_id: String, state: State<'_, AppState>) -> Result<(), ApiError> {
    if state.db.soft_delete_phrase(&phrase_id)?.is_none() {
        return Err(not_found(format!("phrase {phrase_id} not found")));
    }
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    Ok(())
}

// -- Execution (FR-4, FR-5, FR-12) ----------------------------------------- //
#[tauri::command]
pub fn expand_phrase(
    req: ExpandRequest,
    state: State<'_, AppState>,
) -> Result<ExpandResponse, ApiError> {
    let trigger = req
        .trigger
        .clone()
        .ok_or_else(|| ApiError::from(DomainError::Validation("trigger required".into())))?;
    let phrase = state
        .db
        .resolve(&trigger)?
        .ok_or_else(|| ApiError::from(DomainError::NotFound("no phrase matches the given trigger".into())))?;
    let output = state.db.expand(&phrase, &req.inputs)?;
    Ok(ExpandResponse {
        phrase_id: phrase.id,
        abbreviation: phrase.abbreviation,
        content_type: phrase.content_type,
        output,
    })
}

/// M3: resolve a phrase by id, expand it, and type the result into the focused app
/// via the session-aware Injector (enigo on X11/Win/macOS, ydotool on Wayland).
#[tauri::command]
pub fn inject_phrase(
    phrase_id: String,
    inputs: Option<std::collections::HashMap<String, String>>,
    mode: Option<OutputMode>,
    state: State<'_, AppState>,
) -> Result<String, ApiError> {
    let phrase = state
        .db
        .get_phrase(&phrase_id)?
        .ok_or_else(|| not_found(format!("phrase {phrase_id} not found")))?;
    if phrase.deleted_at.is_some() {
        return Err(not_found(format!("phrase {phrase_id} not found")));
    }
    let inputs = inputs.unwrap_or_default();
    let output = state.db.expand(&phrase, &inputs)?;
    match mode.unwrap_or_default() {
        OutputMode::Copy => copy_to_clipboard(&output)?,
        OutputMode::Type => state.injector.type_text(&output)?,
    }
    Ok(output)
}

/// Copy text to the system clipboard (best-effort).
fn copy_to_clipboard(text: &str) -> Result<(), ApiError> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| ApiError::from(DomainError::Validation(format!("clipboard set: {e}"))))?;
    Ok(())
}

// -- Clipboard (FR-9) ------------------------------------------------------ //
#[tauri::command]
pub fn list_clipboard(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ClipboardEntry>, ApiError> {
    Ok(state.db.list_clipboard(limit.unwrap_or(50))?)
}

#[tauri::command]
pub fn add_clipboard(
    payload: ClipboardCreate,
    state: State<'_, AppState>,
) -> Result<ClipboardEntry, ApiError> {
    let entry = ClipboardEntry {
        id: Uuid::new_v4().to_string(),
        text: payload.text,
        captured_at: now_iso(),
    };
    state.db.add_clipboard(&entry)?;
    Ok(entry)
}

// -- Library import/export ------------------------------------------------- //
/// Export the whole library (folders + active phrases) as JSON.
#[tauri::command]
pub fn export_library(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let folders = state.db.list_folders()?;
    let phrases = state.db.list_phrases(None, None, false)?;
    Ok(serde_json::json!({ "folders": folders, "phrases": phrases }))
}

/// Import phrases from CSV: one per line, `abbreviation,text` (text may contain commas).
/// Existing abbreviations are skipped. Returns the number imported.
#[tauri::command]
pub fn import_csv(
    csv_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    let folder_id = match folder_id {
        Some(f) => f,
        None => {
            let f = Folder {
                id: Uuid::new_v4().to_string(),
                name: "Imported".into(),
                parent_id: None,
                created_at: now_iso(),
            };
            state.db.create_folder(&f)?.id
        }
    };

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_text.as_bytes());
    let mut count = 0u32;
    for result in rdr.records() {
        let Ok(record) = result else { continue };
        if record.len() < 2 {
            continue;
        }
        let abbr = record.get(0).unwrap_or("").trim().to_string();
        let text = record.get(1).unwrap_or("").to_string();
        if abbr.is_empty() && text.is_empty() {
            continue;
        }
        let triggers = if abbr.is_empty() {
            Vec::new()
        } else {
            vec![TriggerBinding {
                kind: TriggerType::Autotext,
                abbreviation: Some(abbr.clone()),
                key: None,
            }]
        };
        let name: String = if text.is_empty() {
            abbr.clone()
        } else {
            text.chars().take(80).collect()
        };
        let phrase = Phrase {
            id: Uuid::new_v4().to_string(),
            folder_id: folder_id.clone(),
            name,
            abbreviation: if abbr.is_empty() { None } else { Some(abbr) },
            content_type: ContentType::Plain,
            body: vec![Segment::Text { text }],
            triggers,
            sensitive: false,
            created_at: now_iso(),
            updated_at: now_iso(),
            deleted_at: None,
        };
        if state.db.create_phrase(&phrase).is_ok() {
            count += 1;
        }
    }
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    Ok(count)
}

// -- Autotext (M4) --------------------------------------------------------- //
#[derive(Debug, serde::Serialize)]
pub struct AutotextStatus {
    pub enabled: bool,
    pub running: bool,
    pub events_seen: u64,
    pub abbreviations: usize,
}

#[tauri::command]
pub fn autotext_status(state: State<'_, AppState>) -> AutotextStatus {
    AutotextStatus {
        enabled: state.autotext_enabled.load(Ordering::Relaxed),
        running: state.autotext_running.load(Ordering::Relaxed),
        events_seen: state.autotext_events.load(Ordering::Relaxed),
        abbreviations: state
            .abbrev_index
            .lock()
            .map(|i| i.len())
            .unwrap_or(0),
    }
}

#[tauri::command]
pub fn set_autotext(enabled: bool, state: State<'_, AppState>) -> bool {
    state.autotext_enabled.store(enabled, Ordering::Relaxed);
    enabled
}

// -- Settings (M5) --------------------------------------------------------- //
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, ApiError> {
    Ok(state.db.load_settings()?)
}

#[tauri::command]
pub fn set_settings(
    payload: Settings,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Settings, ApiError> {
    let prev = state.db.load_settings()?;
    let saved = state.db.save_settings(&payload)?;

    // Apply runtime side-effects.
    state.autotext_enabled.store(saved.autotext_enabled, Ordering::Relaxed);
    state.clipboard_enabled.store(saved.clipboard_enabled, Ordering::Relaxed);

    // Popup hotkey changed → re-register.
    if saved.popup_hotkey != prev.popup_hotkey {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        if let Some(sc) = crate::parse_hotkey(&saved.popup_hotkey) {
            let _ = gs.register(sc);
        }
    }

    // Launch-on-login.
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let _ = if saved.autostart { al.enable() } else { al.disable() };

    Ok(saved)
}

// -- helpers --------------------------------------------------------------- //
fn apply_update(existing: Phrase, u: PhraseUpdate) -> DomainResult<Phrase> {
    let mut abbreviation = u.abbreviation.clone().or(existing.abbreviation.clone());
    let triggers = u
        .triggers
        .clone()
        .unwrap_or_else(|| existing.triggers.clone());
    let autotext: Vec<&TriggerBinding> = triggers
        .iter()
        .filter(|t| t.kind == TriggerType::Autotext)
        .collect();
    if autotext.len() > 1 {
        return Err(DomainError::Validation(
            "at most one autotext trigger is allowed".into(),
        ));
    }
    if let Some(t) = autotext.first() {
        abbreviation = t.abbreviation.clone();
    }
    let content_type = u.content_type.unwrap_or(existing.content_type);
    if content_type != ContentType::Plain {
        return Err(DomainError::ReservedContentType(
            "non-plain content types are reserved in v1".into(),
        ));
    }
    Ok(Phrase {
        id: existing.id,
        folder_id: existing.folder_id,
        name: u.name.unwrap_or(existing.name),
        abbreviation,
        content_type,
        body: u.body.unwrap_or(existing.body),
        triggers,
        sensitive: u.sensitive.unwrap_or(existing.sensitive),
        created_at: existing.created_at,
        updated_at: existing.updated_at,
        deleted_at: existing.deleted_at,
    })
}
