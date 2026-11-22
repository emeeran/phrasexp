//! Tauri commands — the frontend→Rust API surface, over Tauri IPC (no HTTP). Each
//! returns `Result<T, ApiError>`; `ApiError` serializes to `{ code, message }` which
//! the frontend inspects.

use std::sync::atomic::Ordering;

use base64::Engine as _;
use tauri::{Emitter, LogicalSize, Manager, Size, State};
use uuid::Uuid;

use crate::domain::models::*;
use crate::domain::{now_iso, ApiError, DomainError, DomainResult};
use crate::AppState;

fn not_found(msg: impl Into<String>) -> ApiError {
    ApiError::from(DomainError::NotFound(msg.into()))
}

// -- Folders --------------------------------------------------------------- //
#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, ApiError> {
    Ok(state.db.list_folders()?)
}

#[tauri::command]
pub fn create_folder(
    payload: FolderCreate,
    state: State<'_, AppState>,
) -> Result<Folder, ApiError> {
    let folder = Folder {
        id: Uuid::new_v4().to_string(),
        name: payload.name,
        parent_id: payload.parent_id,
        created_at: now_iso(),
        enabled: true,
    };
    Ok(state.db.create_folder(&folder)?)
}

#[tauri::command]
pub fn delete_folder(folder_id: String, state: State<'_, AppState>) -> Result<(), ApiError> {
    if state.db.delete_folder(&folder_id)? {
        Ok(())
    } else {
        Err(not_found(format!("folder {folder_id} not found")))
    }
}

/// Common post-library-change tail: rebuild the abbreviation index, refresh the
/// app-poll flag (app-scoped phrases may have changed), re-register per-phrase
/// hotkeys when an AppHandle is available, and optionally kick off an
/// export-on-change sync. Every command that creates/deletes/edits library
/// content ends with the relevant subset of these.
fn after_library_change(state: &State<'_, AppState>, app: Option<&tauri::AppHandle>, sync: bool) {
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    state.refresh_app_poll_needed();
    // Tell every webview (popup included) the library mutated, so cached lists can
    // refresh on event instead of re-fetching on every focus gain.
    if let Some(app) = app {
        let _ = app.emit("library-changed", ());
        crate::reregister_hotkeys(app);
    }
    if sync {
        spawn_sync(std::sync::Arc::clone(&state.db));
    }
}

/// Enable/disable a folder (mute its phrases without deleting). Rebuilds the abbreviation
/// index + per-phrase hotkeys so a disabled folder stops triggering immediately. Returns the
/// new enabled state.
#[tauri::command]
pub fn set_folder_enabled(
    folder_id: String,
    enabled: bool,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, ApiError> {
    state
        .db
        .set_folder_enabled(&folder_id, enabled)?
        .ok_or_else(|| not_found(format!("folder {folder_id} not found")))?;
    after_library_change(&state, Some(&app), false);
    Ok(enabled)
}

/// Merge `source_id` into `target_id`: bulk-move all phrases (soft-deleted included),
/// then delete the source folder. Returns how many phrases moved. Rebuilds the
/// abbreviation index + hotkeys since phrases may have changed triggering folders.
#[tauri::command]
pub fn merge_folders(
    source_id: String,
    target_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<u32, ApiError> {
    let moved = state
        .db
        .merge_folders(&source_id, &target_id)?
        .ok_or_else(|| not_found(format!("folder {source_id} not found")))?;
    after_library_change(&state, Some(&app), false);
    Ok(moved)
}

// -- Phrases --------------------------------------------------------------- //
#[tauri::command]
pub fn list_phrases(
    folder_id: Option<String>,
    q: Option<String>,
    include_deleted: Option<bool>,
    sort: Option<SortKey>,
    state: State<'_, AppState>,
) -> Result<Vec<PhraseSummary>, ApiError> {
    let mut phrases = state.db.list_phrases(
        folder_id.as_deref(),
        q.as_deref(),
        include_deleted.unwrap_or(false),
    )?;
    match sort.unwrap_or_default() {
        SortKey::Name => phrases.sort_by_key(|p| p.name.to_lowercase()),
        SortKey::Updated => phrases.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        SortKey::Created => phrases.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        SortKey::MostUsed => phrases.sort_by(|a, b| {
            b.expanded_count
                .cmp(&a.expanded_count)
                .then_with(|| b.last_used_at.cmp(&a.last_used_at))
        }),
    }
    Ok(phrases.iter().map(|p| state.db.summary(p)).collect())
}

#[tauri::command]
pub fn get_phrase(phrase_id: String, state: State<'_, AppState>) -> Result<Phrase, ApiError> {
    Ok(state.db.get_active_phrase(&phrase_id)?)
}

/// Batch fetch by id — the popup chooser resolves its 2-5 candidates without the
/// whole-library read of `list_phrases`. Preserves the caller's id order.
#[tauri::command]
pub fn get_phrases(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<PhraseSummary>, ApiError> {
    let phrases = state.db.phrases_by_ids(&ids)?;
    Ok(phrases.iter().map(|p| state.db.summary(p)).collect())
}

#[tauri::command]
pub fn create_phrase(
    mut payload: PhraseCreate,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
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
        app_scope: payload.app_scope,
        expanded_count: 0,
        last_used_at: None,
        case_mode: payload.case_mode.unwrap_or_default(),
        created_at: now_iso(),
        updated_at: now_iso(),
        deleted_at: None,
        tags: payload.tags,
    };
    let saved = state.db.create_phrase(&phrase)?;
    after_library_change(&state, Some(&app), true);
    Ok(saved)
}

#[tauri::command]
pub fn update_phrase(
    phrase_id: String,
    payload: PhraseUpdate,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Phrase, ApiError> {
    let existing = state.db.get_active_phrase(&phrase_id)?;
    let updated = apply_update(existing, payload)?;
    let saved = state.db.update_phrase(&updated)?;
    after_library_change(&state, Some(&app), true);
    Ok(saved)
}

#[tauri::command]
pub fn delete_phrase(
    phrase_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), ApiError> {
    if state.db.soft_delete_phrase(&phrase_id)?.is_none() {
        return Err(not_found(format!("phrase {phrase_id} not found")));
    }
    after_library_change(&state, Some(&app), true);
    Ok(())
}

// -- Images ---------------------------------------------------------------- //
/// Store a pasted image (PNG data URL) under the app data dir; returns the file path
/// the phrase body references.
#[tauri::command]
pub fn save_image(app: tauri::AppHandle, data_url: String) -> Result<String, ApiError> {
    let b64 = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| {
            ApiError::from(DomainError::Validation(
                "image must be a PNG data URL".into(),
            ))
        })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("image decode: {e}"))))?;
    let dir = crate::data_dir(&app).join("images");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("image dir: {e}"))))?;
    let path = dir.join(format!("{}.png", Uuid::new_v4()));
    std::fs::write(&path, bytes)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("image write: {e}"))))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Read one of OUR stored images back as a data URL (editor preview only) — rejects
/// anything outside the images dir so the command can't probe arbitrary files.
#[tauri::command]
pub fn read_image(app: tauri::AppHandle, path: String) -> Result<String, ApiError> {
    let dir = crate::data_dir(&app).join("images");
    let p = std::path::Path::new(&path);
    if !p.starts_with(&dir) || p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("png")) {
        return Err(ApiError::from(DomainError::Validation(
            "not an app-stored image".into(),
        )));
    }
    let bytes = std::fs::read(p)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("image read: {e}"))))?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

// -- Execution ------------------------------------------------------------- //
#[tauri::command]
pub async fn expand_phrase(
    req: ExpandRequest,
    state: State<'_, AppState>,
) -> Result<ExpandResponse, ApiError> {
    let trigger = req.trigger.clone();
    let phrase = state.db.resolve(&trigger)?.ok_or_else(|| {
        ApiError::from(DomainError::NotFound(
            "no phrase matches the given trigger".into(),
        ))
    })?;
    let (output, _images) = state.db.expand(&phrase, &req.inputs).await?;
    Ok(ExpandResponse {
        phrase_id: phrase.id,
        abbreviation: phrase.abbreviation,
        content_type: phrase.content_type,
        output,
    })
}

/// Resolve a phrase by id, expand it, and type the result into the focused app
/// via the session-aware Injector (enigo on X11/Win/macOS, ydotool on Wayland).
#[tauri::command]
pub async fn inject_phrase(
    phrase_id: String,
    inputs: Option<std::collections::HashMap<String, String>>,
    mode: Option<OutputMode>,
    state: State<'_, AppState>,
) -> Result<String, ApiError> {
    let phrase = state.db.get_active_phrase(&phrase_id)?;
    let inputs = inputs.unwrap_or_default();
    let chunks = state
        .db
        .expand_chunks_with_clip(&phrase, &inputs, None)
        .await?;
    let (output, _images) = OutChunk::flatten(&chunks);
    match mode.unwrap_or_default() {
        OutputMode::Copy => copy_to_clipboard(&output)?,
        OutputMode::Type => {
            crate::triggers::inject::deliver_chunks(&*state.injector, phrase.content_type, &chunks)?
        }
    }
    // Usage stats (1.4): count manual popup/copy injections too.
    let _ = state.db.record_usage(&phrase_id);
    Ok(output)
}

/// Resolve a trigger to its phrase WITHOUT expanding — lets the UI detect inputform
/// fields and collect them before expansion (used by the Test panel). Returns None if
/// no non-deleted phrase matches the trigger.
#[tauri::command]
pub fn resolve_phrase(
    trigger: TriggerRequest,
    state: State<'_, AppState>,
) -> Result<Option<Phrase>, ApiError> {
    Ok(state.db.resolve(&trigger)?)
}

/// Copy text to the system clipboard (best-effort).
fn copy_to_clipboard(text: &str) -> Result<(), ApiError> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| ApiError::from(DomainError::Validation(format!("clipboard set: {e}"))))?;
    Ok(())
}

// -- Clipboard ------------------------------------------------------------- //
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
    let cap = state.clipboard_max_entries.load(Ordering::Relaxed) as usize;
    state.db.add_clipboard(&entry, cap)?;
    Ok(entry)
}

#[tauri::command]
pub fn clear_clipboard(state: State<'_, AppState>) -> Result<(), ApiError> {
    state.db.clear_clipboard()?;
    Ok(())
}

#[tauri::command]
pub fn reset_database(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), ApiError> {
    state.db.reset_database()?;
    after_library_change(&state, Some(&app), false);
    Ok(())
}

#[tauri::command]
pub fn folder_counts(state: State<'_, AppState>) -> Result<Vec<(String, u32)>, ApiError> {
    Ok(state.db.folder_counts()?)
}

/// Test the AI connection with a simple prompt.
#[tauri::command]
pub async fn test_ai(state: State<'_, AppState>) -> Result<String, ApiError> {
    let settings = state.db.load_settings()?;
    let result = crate::triggers::ai::complete(&settings, "Say hello in one word.").await;
    match result {
        Ok(text) => Ok(format!("AI ok — response: {text}")),
        Err(e) => Err(ApiError::from(e)),
    }
}

/// List the user-editable AI action presets (builtins seeded on first run).
#[tauri::command]
pub fn list_ai_actions(state: State<'_, AppState>) -> Result<Vec<AiAction>, ApiError> {
    Ok(state.db.list_ai_actions()?)
}

/// Create or update an AI action. Builtin actions are editable but stay builtin.
#[tauri::command]
pub fn save_ai_action(state: State<'_, AppState>, action: AiAction) -> Result<AiAction, ApiError> {
    Ok(state.db.save_ai_action(&action)?)
}

/// Delete a custom AI action. Builtin actions cannot be deleted.
#[tauri::command]
pub fn delete_ai_action(state: State<'_, AppState>, action_id: String) -> Result<(), ApiError> {
    Ok(state.db.delete_ai_action(&action_id)?)
}

/// Generate `count` rewording variations of `text` via the AI endpoint (PhraseExpress
/// v18-style template variation — expansion then picks one at random via a Pick macro).
#[tauri::command]
pub async fn ai_variations(
    state: State<'_, AppState>,
    text: String,
    count: usize,
) -> Result<Vec<String>, ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::from(DomainError::Validation(
            "Nothing to vary — write some text first".into(),
        )));
    }
    let count = count.clamp(1, 5);
    let settings = state.db.load_settings()?;
    let prompt = format!(
        "Generate {count} distinct variations of the text below. Keep the meaning and \
         approximate length; naturally vary wording and tone. Separate each variation \
         with a line containing only ---. Return nothing else — no numbering, no \
         commentary.\n\nText:\n{text}"
    );
    let raw = crate::triggers::ai::complete(&settings, &prompt).await?;
    let vars = parse_variations(&raw, count);
    if vars.is_empty() {
        return Err(ApiError::from(DomainError::Validation(
            "AI returned no usable variations — try again or adjust the model".into(),
        )));
    }
    Ok(vars)
}

// -- Citations -------------------------------------------------------------- //

/// Look up structured citation metadata on Crossref for a DOI, URL, or raw
/// title (free, no key; runs Rust-side so capabilities stay untouched).
#[tauri::command]
pub async fn fetch_citation_metadata(
    source: String,
) -> Result<crate::domain::citation::CiteMeta, ApiError> {
    if source.trim().is_empty() {
        return Err(ApiError::from(DomainError::Validation(
            "Enter a DOI, URL, or title first".into(),
        )));
    }
    Ok(crate::domain::citation::fetch_metadata(&source).await?)
}

/// Format already-fetched metadata into a house style (apa | mla | chicago) —
/// the same pure formatter the {cite} macro uses at expansion time.
#[tauri::command]
pub fn format_citation(
    meta: crate::domain::citation::CiteMeta,
    style: String,
) -> Result<String, ApiError> {
    Ok(crate::domain::citation::format_citation(&meta, &style)?)
}

/// Split the variations response on `---` separators, trimming empties. Pure so tests
/// can pin the parsing without a network. (A variation containing a literal `---`
/// would split early — acceptable for natural-language body text.)
fn parse_variations(raw: &str, count: usize) -> Vec<String> {
    raw.split("---")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .take(count)
        .collect()
}
/// Run an AI action on the selection captured by the selection-AI hotkey. The popup
/// hides before calling this; on success the popup is re-shown in result mode with
/// the rewrite and the choice of Replace / Insert / Retry / Discard (ai_decide) —
/// nothing is pasted until the user decides. The user's prior clipboard is restored
/// on every terminal path.
#[tauri::command]
pub async fn run_ai_action(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action_id: String,
    params: Option<std::collections::HashMap<String, String>>,
) -> Result<String, ApiError> {
    // Take the context out up front — no lock may be held across the await.
    let ctx = state
        .ai_selection
        .lock()
        .ok()
        .and_then(|mut g| g.take())
        .ok_or_else(|| ApiError::from(DomainError::Validation("No pending selection".into())))?;
    state
        .ai_cancel
        .store(false, std::sync::atomic::Ordering::Relaxed);
    // Both failure paths restore the selection copy + prior clipboard; success keeps
    // them (old_clipboard rides into AiPending so ai_decide can restore it later).
    let restore_on_failure = || {
        restore_clipboard(Some(ctx.selection.clone()));
        restore_clipboard(ctx.old_clipboard.clone());
    };
    match stream_ai_action(&app, &state, &action_id, &ctx.selection, params.clone()).await {
        Ok(result) => {
            if let Ok(mut g) = state.ai_result.lock() {
                *g = Some(crate::AiPending {
                    action_id,
                    result: result.clone(),
                    selection: ctx.selection.clone(),
                    params,
                    old_clipboard: ctx.old_clipboard,
                });
            }
            // Bring the popup back in result mode, then hand over the rewrite —
            // plus the original selection so the webview can show a diff preview.
            crate::show_popup(&app);
            let _ = app.emit(
                "ai-result",
                serde_json::json!({ "result": result, "original": ctx.selection }),
            );
            Ok(result)
        }
        Err(e) if e.to_string().contains(crate::triggers::ai::CANCELLED_MSG) => {
            // User cancelled mid-stream: same cleanup as a discard — the user's
            // text is untouched, their prior clipboard comes back, the popup
            // stands down (no result view to show).
            restore_on_failure();
            let _ = app.emit("ai-cancelled", ());
            Ok(String::new())
        }
        Err(e) => {
            // Nothing was pasted: put back the selection copy so the user's target
            // text is still reachable, then restore their original clipboard. Show
            // the popup — it's the surface the user is looking at (the main window
            // is usually hidden to tray, so phrase-error toasts are never seen).
            restore_on_failure();
            crate::show_popup(&app);
            let _ = app.emit("ai-result-error", e.to_string());
            Err(ApiError::from(e))
        }
    }
}

/// Resolve one AI decision from the result popup.
/// - `replace` — paste over the still-active selection (target regains focus when
///   the popup hides, so the rewrite lands in place of the original).
/// - `insert` — collapse the selection to its end (Right arrow), then paste: the
///   original survives, the rewrite is added after it.
/// - `copy` — put the rewrite on the clipboard and stop; the user's prior clipboard
///   is deliberately NOT restored, since the rewrite is meant to be taken from there.
/// - `retry` — run the same action again on the same selection; the pending result
///   stays until the new one replaces it, so a failed retry keeps the old output.
/// - `rerun` — like retry, but through a DIFFERENT builtin (the result view's
///   quick chips): `text` carries the target action id, and the pending slot's
///   action + params adopt it so a later retry re-runs the chip action.
/// - `discard` — nothing was pasted; just restore the user's prior clipboard.
/// `text` optionally overrides what replace/insert/copy act on — the webview sends
/// it when the user picked one option out of a multi-option response.
#[tauri::command]
pub async fn ai_decide(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    decision: String,
    text: Option<String>,
) -> Result<String, ApiError> {
    // Peek, run, then take — so a retry error leaves the previous result decidable.
    let pending = state
        .ai_result
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .ok_or_else(|| ApiError::from(DomainError::Validation("No pending AI result".into())))?;
    // An override narrower than the pending result must still carry content.
    let chosen = match text {
        Some(ref t) if !t.trim().is_empty() => t.clone(),
        _ => pending.result.clone(),
    };
    match decision.as_str() {
        "replace" | "insert" => {
            let _ = state.ai_result.lock().map(|mut g| *g = None);
            if decision == "insert" {
                state.injector.move_right(1)?;
            }
            state.injector.paste_text(&chosen)?;
            restore_clipboard(pending.old_clipboard);
            Ok(chosen)
        }
        "copy" => {
            let _ = state.ai_result.lock().map(|mut g| *g = None);
            copy_to_clipboard(&chosen)?;
            Ok(chosen)
        }
        "retry" => {
            state
                .ai_cancel
                .store(false, std::sync::atomic::Ordering::Relaxed);
            match stream_ai_action(
                &app,
                &state,
                &pending.action_id,
                &pending.selection,
                pending.params.clone(),
            )
            .await
            {
                Ok(fresh) => {
                    let result = fresh.clone();
                    if let Ok(mut g) = state.ai_result.lock() {
                        *g = Some(crate::AiPending { result, ..pending });
                    }
                    Ok(fresh)
                }
                Err(e) if e.to_string().contains(crate::triggers::ai::CANCELLED_MSG) => {
                    // Cancelled mid-retry: the old pending result is still in its
                    // slot, but the popup stands down like a first-run cancel.
                    let _ = app.emit("ai-cancelled", ());
                    Ok(String::new())
                }
                Err(e) => Err(ApiError::from(e)),
            }
        }
        "rerun" => {
            let Some(action_id) = text.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
                return Err(ApiError::from(DomainError::Validation(
                    "rerun needs an action id".into(),
                )));
            };
            state
                .ai_cancel
                .store(false, std::sync::atomic::Ordering::Relaxed);
            match stream_ai_action(&app, &state, action_id, &pending.selection, None).await {
                Ok(fresh) => {
                    if let Ok(mut g) = state.ai_result.lock() {
                        *g = Some(crate::AiPending {
                            action_id: action_id.to_string(),
                            // Chip builtins carry no interactive tokens; drop the
                            // origin action's params so they can't leak across.
                            params: None,
                            result: fresh.clone(),
                            ..pending
                        });
                    }
                    Ok(fresh)
                }
                Err(e) if e.to_string().contains(crate::triggers::ai::CANCELLED_MSG) => {
                    // Same stand-down as a cancelled retry.
                    let _ = app.emit("ai-cancelled", ());
                    Ok(String::new())
                }
                // Peek-not-take (shared pending above): a failed rerun keeps the
                // previous result decidable, same as a failed retry.
                Err(e) => Err(ApiError::from(e)),
            }
        }
        "discard" => {
            let _ = state.ai_result.lock().map(|mut g| *g = None);
            restore_clipboard(pending.old_clipboard);
            Ok(pending.result)
        }
        other => Err(ApiError::from(DomainError::Validation(format!(
            "unknown ai decision: {other}"
        )))),
    }
}

/// Load settings + the action, substitute the selection (and the interactive
/// `{token}` parameters, when the template asks for any) into the template, and
/// stream the completion — each delta is emitted to the webview as `ai-delta`
/// (payload = accumulated text) so the popup renders progressively. Shared by the
/// first run and every retry.
async fn stream_ai_action(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    action_id: &str,
    selection: &str,
    params: Option<std::collections::HashMap<String, String>>,
) -> DomainResult<String> {
    let settings = state.db.load_settings()?;
    let action = state
        .db
        .get_ai_action(action_id)?
        .ok_or_else(|| DomainError::NotFound(format!("ai action {action_id} not found")))?;
    let params = required_params(&action.prompt_template, params.as_ref())?;
    let prompt = substitute_template(&action.prompt_template, selection, &params);
    let cancel = std::sync::Arc::clone(&state.ai_cancel);
    let emitter = app.clone();
    let mut acc = String::new();
    let result = crate::triggers::ai::complete_stream(&settings, &prompt, &cancel, |delta| {
        acc.push_str(delta);
        // The webview APPENDS each delta to its own accumulator — re-emitting the
        // whole accumulated string per token was O(n²) IPC churn on long answers.
        let _ = emitter.emit("ai-delta", delta);
    })
    .await?;
    Ok(result)
}

/// Every distinct interactive token, in template order. Multi-token templates
/// (e.g. Summarize's `{length}` + `{format}`) prompt once per token.
fn param_tokens(template: &str) -> Vec<String> {
    let mut rest = template;
    let mut out: Vec<String> = Vec::new();
    while let Some(start) = rest.find('{') {
        let Some(end_rel) = rest[start..].find('}') else {
            break;
        };
        let end = start + end_rel;
        let token = &rest[start + 1..end];
        if is_param_token(token) && !out.iter().any(|t| t == token) {
            out.push(token.to_string());
        }
        rest = &rest[end + 1..];
    }
    out
}

/// A brace token is an interactive param (substitutable from answers) when it's
/// non-empty, alphanumeric/underscore, and not the reserved `{clipboard}`/`{selection}`.
fn is_param_token(token: &str) -> bool {
    !token.is_empty()
        && token != "clipboard"
        && token != "selection"
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate the params against the template's placeholders: every interactive
/// token needs a non-empty answer (the frontend prompts; reaching here without
/// one means a non-interactive caller). Returns the answers keyed by token
/// (empty map when the template has none).
fn required_params(
    template: &str,
    params: Option<&std::collections::HashMap<String, String>>,
) -> DomainResult<std::collections::HashMap<String, String>> {
    let empty = std::collections::HashMap::new();
    let provided = params.unwrap_or(&empty);
    let mut out = std::collections::HashMap::new();
    for token in param_tokens(template) {
        let value = provided
            .get(&token)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                DomainError::Validation(format!("This action needs a value for {{{token}}}"))
            })?;
        out.insert(token, value.to_string());
    }
    Ok(out)
}

/// Substitute `{clipboard}`/`{selection}` with the captured selection and any
/// remaining `{token}` with its keyed param answer. Unknown/unclosed braces
/// survive literally, so prose containing braces isn't mangled. Pure — unit-tested.
fn substitute_template(
    template: &str,
    selection: &str,
    params: &std::collections::HashMap<String, String>,
) -> String {
    let base = template
        .replace("{clipboard}", selection)
        .replace("{selection}", selection);
    let mut out = String::with_capacity(base.len());
    let mut rest = base.as_str();
    while let Some(start) = rest.find('{') {
        match rest[start..].find('}') {
            Some(end_rel) => {
                let end = start + end_rel;
                let token = &rest[start + 1..end];
                if is_param_token(token) {
                    if let Some(p) = params.get(token) {
                        out.push_str(&rest[..start]);
                        out.push_str(p);
                        rest = &rest[end + 1..];
                        continue;
                    }
                    // Substitutable but unanswered — keep it literally (required_params
                    // rejects this path upstream; this is just the safe fallback).
                }
                // Known placeholder already replaced, or not a token: keep literally.
                out.push_str(&rest[..=end]);
                rest = &rest[end + 1..];
            }
            None => {
                // Unclosed brace: emit the remainder verbatim.
                out.push_str(rest);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Cancel an in-flight streaming AI request. The stream stops at the next chunk;
/// `run_ai_action`/retry recognize the cancel, restore the clipboard, and emit
/// `ai-cancelled` so the popup stands down.
#[tauri::command]
pub fn ai_cancel(state: State<'_, AppState>) {
    state
        .ai_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Put `clip_text` back on the clipboard (no-op on None) — used to hand back the
/// user's prior clipboard after the selection-AI flow is done with it.
fn restore_clipboard(clip_text: Option<String>) {
    if let Some(prev) = clip_text {
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(&prev);
        }
    }
}

// -- File-based sync (Settings → Data) ------------------------------------- //

/// Header of the sync snapshot, for the settings UI's "last sync" line.
#[derive(Debug, serde::Serialize)]
pub struct SyncStatus {
    pub dir: String,
    pub device: Option<String>,
    pub exported_at: Option<String>,
    pub phrases: Option<usize>,
}

/// Pull + push against the configured sync folder, then rebuild the trigger index
/// (imported/updated phrases must start firing immediately).
#[tauri::command]
pub fn sync_now(state: State<'_, AppState>) -> Result<crate::sync::SyncReport, ApiError> {
    let report = crate::sync::sync_now(&state.db)?;
    let _ = crate::triggers::rebuild_index(&state.db, &state.abbrev_index);
    state.refresh_app_poll_needed();
    Ok(report)
}

/// Point sync at a folder (empty string disables). Takes effect on the next
/// sync_now / startup sync.
#[tauri::command]
pub fn sync_set_dir(state: State<'_, AppState>, dir: String) -> Result<(), ApiError> {
    let mut s = state.db.load_settings()?;
    s.sync_dir = dir.trim().to_string();
    state.db.save_settings(&s)?;
    Ok(())
}

/// Set the sync snapshot passphrase (empty = plaintext snapshot). Every device
/// sharing the sync folder must use the SAME passphrase — a mismatch is a named
/// decryption error at merge time, never a silent library overwrite.
#[tauri::command]
pub fn sync_set_passphrase(state: State<'_, AppState>, passphrase: String) -> Result<(), ApiError> {
    let mut s = state.db.load_settings()?;
    s.sync_passphrase = passphrase;
    state.db.save_settings(&s)?;
    Ok(())
}

/// Current sync configuration + snapshot header (None fields when no snapshot
/// exists yet — sync is configured but hasn't run).
#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus, ApiError> {
    let settings = state.db.load_settings()?;
    let dir = settings.sync_dir;
    let (mut device, mut exported_at, mut phrases) = (None, None, None);
    if !dir.trim().is_empty() {
        let h = crate::sync::read_snapshot_header(std::path::Path::new(&dir));
        device = h.device;
        exported_at = h.exported_at;
        phrases = h.phrase_count;
    }
    Ok(SyncStatus {
        dir,
        device,
        exported_at,
        phrases,
    })
}

/// Auto-sync coalescing: a burst of library mutations (import, rapid saves)
/// collapses into ONE sync ~2 s after the last request, instead of a whole-library
/// snapshot rewrite per edit. Manual "Sync now" bypasses this and runs immediately.
static SYNC_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SYNC_WORKER_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Best-effort sync after a library mutation: never blocks the command, never
/// surfaces errors (a failed auto-sync just means the file is one beat behind;
/// Settings → Data → "Sync now" forces it).
fn spawn_sync(db: std::sync::Arc<crate::db::Db>) {
    use std::sync::atomic::Ordering;
    SYNC_REQUESTED.store(true, Ordering::Relaxed);
    // One worker at a time; it keeps looping while requests keep arriving and
    // exits after a quiet window. The double-check on exit closes the race where
    // a request lands between the last swap and the running-flag clear.
    if SYNC_WORKER_RUNNING.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if SYNC_REQUESTED.swap(false, Ordering::Relaxed) {
            if let Err(e) = crate::sync::sync_now(&db) {
                crate::debug_log(&format!("auto sync: {e}"));
            }
            continue;
        }
        SYNC_WORKER_RUNNING.store(false, Ordering::Relaxed);
        if SYNC_REQUESTED.swap(false, Ordering::Relaxed) {
            SYNC_WORKER_RUNNING.store(true, Ordering::Relaxed);
            continue;
        }
        break;
    });
}

/// Fetch available models from the provider's /models endpoint.
#[tauri::command]
pub async fn list_ai_models(state: State<'_, AppState>) -> Result<Vec<String>, ApiError> {
    let settings = state.db.load_settings()?;
    if settings.ai_api_key.is_empty() {
        return Err(ApiError::from(DomainError::Validation(
            "Enter an API key first".into(),
        )));
    }
    let base = settings.ai_base_url.trim_end_matches('/');
    let url = format!("{base}/models");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::from(DomainError::Validation(format!("http client: {e}"))))?;
    let resp = client
        .get(&url)
        .bearer_auth(&settings.ai_api_key)
        .send()
        .await
        .map_err(|e| ApiError::from(DomainError::Validation(format!("request: {e}"))))?;
    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError::from(DomainError::Validation(format!("parse: {e}"))))?;
    let models = val["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

// -- Library import/export ------------------------------------------------- //
/// Export the whole library (folders + active phrases) as JSON. The payload includes
/// `sensitive_count` so the UI can warn that `list_phrases` returns DECRYPTED bodies —
/// the export file is NOT encrypted, so treat it as sensitive.
#[tauri::command]
pub fn export_library(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let folders = state.db.list_folders()?;
    let phrases = state.db.list_phrases(None, None, false)?;
    let sensitive_count = phrases.iter().filter(|p| p.sensitive).count();
    Ok(serde_json::json!({
        "folders": folders,
        "phrases": phrases,
        "sensitive_count": sensitive_count,
    }))
}

// -- Format exporters (3.3): Espanso YAML / TextExpander CSV / plain CSV ------- //
// Foreign formats carry TEXT-ONLY snippets: macros, images, and AI bodies have no
// honest representation there, so phrases containing them are SKIPPED and the
// count is reported. Regex triggers and app scoping likewise don't transfer.

/// One exportable row: trigger/abbreviation, replacement text, label.
pub(crate) struct ExportRow {
    trigger: String,
    label: String,
    text: String,
}

/// Distill phrases into export rows; returns (rows, skipped_count). Phrases
/// without an abbreviation, with regex triggers, with macro/image/AI bodies, or
/// with empty text are skipped. Pure — unit-tested.
pub(crate) fn export_rows(phrases: &[Phrase]) -> (Vec<ExportRow>, usize) {
    let mut rows = Vec::new();
    let mut skipped = 0usize;
    for p in phrases {
        let Some(abbr) = p.abbreviation.as_deref().filter(|a| !a.is_empty()) else {
            skipped += 1;
            continue;
        };
        let is_regex = p
            .triggers
            .iter()
            .any(|t| t.kind == crate::domain::models::TriggerType::Autotext && t.regex);
        if is_regex {
            skipped += 1;
            continue;
        }
        // Only plain-text bodies transfer; anything else (macro/image segment) can't.
        let mut text = String::new();
        let mut exportable = true;
        for seg in &p.body {
            match seg {
                crate::domain::models::Segment::Text { text: t } => text.push_str(t),
                _ => {
                    exportable = false;
                    break;
                }
            }
        }
        if !exportable || text.trim().is_empty() {
            skipped += 1;
            continue;
        }
        rows.push(ExportRow {
            trigger: abbr.to_string(),
            label: p.name.clone(),
            text,
        });
    }
    (rows, skipped)
}

fn export_payload(text: String, exported: usize, skipped: usize) -> serde_json::Value {
    serde_json::json!({ "text": text, "exported": exported, "skipped": skipped })
}

/// Espanso `matches` YAML — re-importable by Espanso directly (`espanso match`).
#[tauri::command]
pub fn export_espanso(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let phrases = state.db.list_phrases(None, None, false)?;
    let (rows, skipped) = export_rows(&phrases);
    let text = espanso_yaml(&rows);
    Ok(export_payload(text, rows.len(), skipped))
}

/// TextExpander CSV (official columns: abbreviation, snippet, label) — round-trips
/// through the existing `import_textexpander`.
#[tauri::command]
pub fn export_textexpander_csv(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let phrases = state.db.list_phrases(None, None, false)?;
    let (rows, skipped) = export_rows(&phrases);
    Ok(export_payload(textexpander_csv(&rows), rows.len(), skipped))
}

/// Plain CSV (`abbreviation,text` per line) — round-trips through `import_csv`.
#[tauri::command]
pub fn export_csv(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let phrases = state.db.list_phrases(None, None, false)?;
    let (rows, skipped) = export_rows(&phrases);
    Ok(export_payload(plain_csv(&rows), rows.len(), skipped))
}

fn textexpander_csv(rows: &[ExportRow]) -> String {
    let mut w = csv::WriterBuilder::new().from_writer(Vec::new());
    let _ = w.write_record(["abbreviation", "snippet", "label"]);
    for r in rows {
        let _ = w.write_record([&r.trigger, &r.text, &r.label]);
    }
    String::from_utf8_lossy(&w.into_inner().unwrap_or_default()).into_owned()
}

/// Espanso `matches` YAML (pure helper — the command wraps it with counts).
/// serde_yaml emits literal block scalars for multiline `replace` values, so
/// multi-line snippets round-trip without hand-rolled quoting.
fn espanso_yaml(rows: &[ExportRow]) -> String {
    let matches: Vec<serde_yaml::Value> = rows
        .iter()
        .map(|r| {
            serde_yaml::to_value(serde_json::json!({
                "trigger": r.trigger,
                "replace": r.text,
                "label": r.label,
            }))
            .unwrap_or(serde_yaml::Value::Null)
        })
        .collect();
    let mut root = serde_yaml::Mapping::new();
    root.insert(
        serde_yaml::Value::from("matches"),
        serde_yaml::to_value(&matches).unwrap_or(serde_yaml::Value::Null),
    );
    serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).unwrap_or_default()
}

fn plain_csv(rows: &[ExportRow]) -> String {
    let mut w = csv::WriterBuilder::new().from_writer(Vec::new());
    for r in rows {
        let _ = w.write_record([&r.trigger, &r.text]);
    }
    String::from_utf8_lossy(&w.into_inner().unwrap_or_default()).into_owned()
}

/// Import phrases from CSV: one per line, `abbreviation,text` (text may contain commas).
/// Existing abbreviations are skipped. Returns the number imported.
#[tauri::command]
pub fn import_csv(
    csv_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    let folder_id = resolve_folder(&state, folder_id)?;

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_text.as_bytes());
    let mut count = 0u32;
    for result in rdr.records() {
        let Ok(record) = result else { continue };
        if record.len() < 2 {
            continue;
        }
        let abbr = record[0].trim().to_string();
        let text = record[1].to_string();
        if abbr.is_empty() && text.is_empty() {
            continue;
        }
        count += create_import_phrase(&state, &folder_id, &abbr, &text, None)?;
    }
    after_library_change(&state, None, false);
    Ok(count)
}

// -- TextExpander / Espanso import (1.6) ------------------------------------ //
/// One parsed import row: abbreviation, replacement text, optional label.
type ImportRow = (String, String, Option<String>);

/// Parse a TextExpander CSV export (official columns: abbreviation, snippet, label).
/// The header row is detected by name when present; otherwise columns fall back to
/// positional (0=abbr, 1=content). Pure — unit-tested without the DB.
fn parse_textexpander_csv(csv_text: &str) -> Vec<ImportRow> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_text.as_bytes());
    let records: Vec<Vec<String>> = rdr
        .records()
        .filter_map(|r| r.ok())
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
    let mut rows = Vec::new();
    let mut abbr_col = 0usize;
    let mut body_col = 1usize;
    let mut label_col: Option<usize> = None;
    let mut start = 0usize;
    if let Some(first) = records.first() {
        let lower: Vec<String> = first.iter().map(|c| c.trim().to_lowercase()).collect();
        if let Some(i) = lower.iter().position(|c| c == "abbreviation") {
            abbr_col = i;
            body_col = lower
                .iter()
                .position(|c| c == "snippet" || c == "content")
                .unwrap_or(i + 1);
            label_col = lower.iter().position(|c| c == "label" || c == "name");
            start = 1; // header row consumed
        }
    }
    for rec in &records[start..] {
        let abbr = rec
            .get(abbr_col)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let body = rec.get(body_col).cloned().unwrap_or_default();
        if abbr.is_empty() || body.is_empty() {
            continue;
        }
        let label = label_col
            .and_then(|i| rec.get(i))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        rows.push((abbr, body, label));
    }
    rows
}

/// Parse an Espanso matches file (YAML subset): entries under `matches:` with
/// `trigger:` + `replace:` keys, including `|`/`|-`/`>` block scalars. Everything
/// else — forms, vars, script extensions — is IGNORED: only literal replacement
/// text is imported, and this app never interprets Espanso semantics, so nothing
/// imported can execute. Pure — unit-tested without the DB.
fn parse_espanso_yaml(yaml: &str) -> Vec<ImportRow> {
    let mut rows = Vec::new();
    let mut in_matches = false;
    let mut trigger: Option<String> = None;
    let mut replace: Option<String> = None;
    // Indent of the `replace:` key while a block scalar (|/>) is being collected.
    let mut block_key_indent: Option<usize> = None;

    fn flush(
        rows: &mut Vec<ImportRow>,
        trigger: &mut Option<String>,
        replace: &mut Option<String>,
    ) {
        if let (Some(t), Some(r)) = (trigger.take(), replace.take()) {
            let t = t.trim();
            let r = r.trim();
            if !t.is_empty() && !r.is_empty() {
                rows.push((t.to_string(), r.to_string(), None));
            }
        }
    }

    /// Store one `trigger:`/`replace:` key line (with or without the `- ` list
    /// prefix already stripped). Other keys (form/vars/extensions) are ignored;
    /// a match without a literal `replace` is skipped at flush — imported
    /// snippets are inert text only.
    fn store_match_key(
        body: &str,
        indent: usize,
        trigger: &mut Option<String>,
        replace: &mut Option<String>,
        block_key_indent: &mut Option<usize>,
    ) {
        if let Some(v) = body.strip_prefix("trigger:") {
            *trigger = Some(unquote(v));
        } else if let Some(v) = body.strip_prefix("replace:") {
            let v = v.trim();
            if matches!(v, "|" | "|-" | "|+" | ">" | ">-") {
                *block_key_indent = Some(indent);
                *replace = Some(String::new());
            } else if !v.is_empty() {
                *replace = Some(unquote(v));
            }
        }
    }

    for raw in yaml.lines() {
        // Inside a block scalar: blank lines and anything deeper-indented than the
        // `replace:` key are content. Collected as literal lines (the folded `>`
        // variant is imported line-broken rather than space-folded — close enough
        // for snippet text, and honest about being a subset parser).
        if let Some(key_indent) = block_key_indent {
            let line_indent = raw.len() - raw.trim_start().len();
            if raw.trim().is_empty() || line_indent > key_indent {
                if let Some(r) = &mut replace {
                    if !r.is_empty() {
                        r.push('\n');
                    }
                    // Strip the standard 2-space content indent relative to the key
                    // (deeper content keeps its extra spaces). Byte slicing is safe:
                    // the prefix is only treated as indent when it's all whitespace.
                    let strip = key_indent + 2;
                    let content = match raw.get(..strip.min(raw.len())) {
                        Some(prefix) if prefix.trim().is_empty() => {
                            raw.get(strip.min(raw.len())..).unwrap_or(raw.trim_start())
                        }
                        _ => raw.trim_start(),
                    };
                    r.push_str(content.trim_end());
                }
                continue;
            }
            block_key_indent = None;
            flush(&mut rows, &mut trigger, &mut replace);
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        if trimmed == "matches:" {
            in_matches = true;
            continue;
        }
        if !in_matches {
            continue;
        }
        // A new list item ends the previous match entry.
        if let Some(rest) = trimmed.strip_prefix("- ") {
            flush(&mut rows, &mut trigger, &mut replace);
            store_match_key(
                rest.trim(),
                indent,
                &mut trigger,
                &mut replace,
                &mut block_key_indent,
            );
            // Other first-keys (form:, word:, ...) — entry ignored via empty replace.
            continue;
        }
        store_match_key(
            trimmed,
            indent,
            &mut trigger,
            &mut replace,
            &mut block_key_indent,
        );
    }
    flush(&mut rows, &mut trigger, &mut replace);
    rows
}

/// Strip matching single/double quotes from a YAML scalar value.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// Import a TextExpander CSV export.
#[tauri::command]
pub fn import_textexpander(
    csv_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    let folder_id = resolve_folder(&state, folder_id)?;
    let mut count = 0u32;
    for (abbr, body, label) in parse_textexpander_csv(&csv_text) {
        count += create_import_phrase(&state, &folder_id, &abbr, &body, label.as_deref())?;
    }
    after_library_change(&state, None, false);
    Ok(count)
}

/// Import an Espanso matches YAML file (literal trigger/replace pairs only).
#[tauri::command]
pub fn import_espanso(
    yaml_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    let folder_id = resolve_folder(&state, folder_id)?;
    let mut count = 0u32;
    for (trigger, body, label) in parse_espanso_yaml(&yaml_text) {
        count += create_import_phrase(&state, &folder_id, &trigger, &body, label.as_deref())?;
    }
    after_library_change(&state, None, false);
    Ok(count)
}

/// Create a phrase with a single text segment + autotext trigger. `label` (from
/// snippet metadata) overrides the name, which otherwise falls back to the first
/// 80 chars of the text, or the abbreviation for empty text. Returns 1 on success,
/// 0 on skip (e.g. duplicate id).
fn create_import_phrase(
    state: &State<'_, AppState>,
    folder_id: &str,
    abbr: &str,
    text: &str,
    label: Option<&str>,
) -> Result<u32, ApiError> {
    let name: String = match label {
        Some(l) => l.chars().take(80).collect(),
        None if text.is_empty() => abbr.to_string(),
        None => text.chars().take(80).collect(),
    };
    let phrase = simple_phrase(folder_id.to_string(), abbr, text, name);
    if state.db.create_phrase(&phrase).is_ok() {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Import AutoHotKey hotstrings (::trigger::replacement format).
/// Skips exclusion hotstrings (no replacement), directives, and comments.
#[tauri::command]
pub fn import_ahk(
    ahk_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    let folder_id = resolve_folder(&state, folder_id)?;

    let mut count = 0u32;
    for line in ahk_text.lines() {
        let line = line.trim();
        // Skip directives, comments, blank lines.
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        // Match ::trigger::replacement
        let Some(rest) = line.strip_prefix("::") else {
            continue;
        };
        let Some(idx) = rest.find("::") else {
            continue;
        };
        let abbr = rest[..idx].trim();
        let replacement_raw = &rest[idx + 2..];
        // Strip inline AHK comments (double-space + semicolon).
        let replacement = replacement_raw
            .split("  ;")
            .next()
            .unwrap_or(replacement_raw)
            .trim();
        if abbr.is_empty() || replacement.is_empty() {
            continue;
        }
        count += create_import_phrase(&state, &folder_id, abbr, replacement, None)?;
    }
    after_library_change(&state, None, false);
    Ok(count)
}

/// Import from an aText backup (.atext). Best-effort: the format embeds binary framing
/// within text values, so extraction is approximate. Finds abbreviation+content pairs
/// by detecting readable text separated by binary gaps (which become multi-space when
/// non-printable bytes are stripped). Returns the number imported.
#[tauri::command]
pub fn import_atext(
    atext_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    // aText exports both binary .atext AND JSON. If the input is JSON, delegate to the JSON
    // importer (it understands aText's numeric-key schema); the binary regex parser below
    // would otherwise yield 0 phrases on a JSON file.
    let is_json = atext_text
        .trim_start()
        .trim_start_matches('\u{feff}')
        .starts_with(['{', '[']);
    if is_json {
        return import_json(atext_text, folder_id, state);
    }
    let folder_id = resolve_folder(&state, folder_id)?;

    // Strip non-printable bytes → spaces (binary framing becomes 2+ space gaps).
    let text: String = atext_text
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();

    // Find pairs: abbreviation (2-20 chars, starts with letter) + 2+ spaces + content (5+).
    let re = regex::Regex::new(
        r"([A-Za-z][A-Za-z0-9_,;:!/\.@#\-]{1,19}) {2,}([A-Za-z][A-Za-z0-9 ,.\!\?;\:\-()/\n']{4,200})",
    )
    .map_err(|e| ApiError::from(DomainError::Validation(format!("regex: {e}"))))?;

    let mut count = 0u32;
    let mut seen = std::collections::HashSet::new();

    for cap in re.captures_iter(&text) {
        let abbr = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        let content = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();

        // Aggressive filter: skip noise (UUIDs, hex, base64, JSON, too short/long).
        if abbr.len() < 2 || content.len() < 5 || content.len() > 200 {
            continue;
        }
        if abbr
            .chars()
            .filter(|c| c.is_ascii_hexdigit() || *c == '-')
            .count()
            == abbr.len()
        {
            continue; // UUID-like
        }
        if content.contains("{\"") || content.contains("=\"") || content.contains("BORw0") {
            continue; // JSON / base64
        }
        // Content should be mostly readable (>60% alpha+space).
        let readable = content
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .count();
        if (readable as f64 / content.len() as f64) < 0.6 {
            continue;
        }
        if seen.contains(abbr) {
            continue;
        }
        seen.insert(abbr.to_string());

        count += create_import_phrase(&state, &folder_id, abbr, content, None)?;
    }

    after_library_change(&state, None, false);
    Ok(count)
}

/// Parse the first JSON value in `text`, tolerating a leading UTF-8 BOM and any trailing
/// bytes (aText appends a NUL; `serde_json::from_str` rejects both). Used by `import_json`.
fn parse_first_json_value(text: &str) -> Result<serde_json::Value, serde_json::Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut de = serde_json::Deserializer::from_str(text);
    <serde_json::Value as serde::Deserialize>::deserialize(&mut de)
}

/// Parse an aText JSON backup (its numeric-key schema):
///   { "4":[ { "2": group_name, "13":[ { "1":[triggers], "4": content, "12": desc? } ] } ] }
/// Each group becomes a folder; each snippet becomes a plain-text phrase with its first trigger
/// as the abbreviation. Strips aText's internal `<atext ...>...</atext>` clipboard-macro markers
/// from content. Returns (folders, phrases) with phrases linked to their group's folder id.
fn parse_atext_library(value: &serde_json::Value) -> Result<(Vec<Folder>, Vec<Phrase>), ApiError> {
    let groups = value.get("4").and_then(|v| v.as_array()).ok_or_else(|| {
        ApiError::from(DomainError::Validation(
            "unrecognized JSON: expected a PhraseXp library, a phrase array, or an aText backup"
                .into(),
        ))
    })?;
    let tag_re = regex::Regex::new(r#"<atext[^>]*>|</atext>"#)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("regex: {e}"))))?;
    let mut folders = Vec::new();
    let mut phrases = Vec::new();
    for (gi, group) in groups.iter().enumerate() {
        let gname = group
            .get("2")
            .and_then(|v| v.as_str())
            .unwrap_or("Imported")
            .to_string();
        // Synthetic folder id — import_json remaps it to a fresh UUID on insert.
        let folder_id = format!("__atext_group_{gi}");
        folders.push(Folder {
            id: folder_id.clone(),
            name: gname,
            parent_id: None,
            created_at: now_iso(),
            enabled: true,
        });
        if let Some(snippets) = group.get("13").and_then(|v| v.as_array()) {
            for snip in snippets {
                let abbr = snip
                    .get("1")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let content = snip.get("4").and_then(|v| v.as_str()).unwrap_or("");
                let content = tag_re.replace_all(content, "").trim().to_string();
                if abbr.is_empty() && content.is_empty() {
                    continue;
                }
                let name = snip
                    .get("12")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        if content.is_empty() {
                            abbr.to_string()
                        } else {
                            content.chars().take(80).collect()
                        }
                    });
                phrases.push(simple_phrase(folder_id.clone(), abbr, &content, name));
            }
        }
    }
    Ok((folders, phrases))
}

/// Import a PhraseXp library export — `{"folders":[...],"phrases":[...]}` (the format produced
/// by `export_library`). Also accepts a bare `[phrase,...]` array. Preserves folder hierarchy
/// (rooted at `folder_id` when given) and full phrase bodies (macros, triggers); assigns fresh
/// IDs and remaps folder + nested-phrase references so re-importing can't collide. Phrases whose
/// abbreviation already exists are skipped. Returns the number of phrases imported.
#[tauri::command]
pub fn import_json(
    json_text: String,
    folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, ApiError> {
    #[derive(serde::Deserialize)]
    struct LibraryImport {
        #[serde(default)]
        folders: Vec<Folder>,
        #[serde(default)]
        phrases: Vec<Phrase>,
    }
    // Strip BOM + tolerate trailing bytes (aText appends a NUL), then dispatch on shape.
    let value = parse_first_json_value(&json_text)
        .map_err(|e| ApiError::from(DomainError::Validation(format!("invalid JSON: {e}"))))?;
    let lib = match &value {
        serde_json::Value::Array(_) => {
            let phrases: Vec<Phrase> = serde_json::from_value(value.clone()).map_err(|e| {
                ApiError::from(DomainError::Validation(format!(
                    "invalid phrase array: {e}"
                )))
            })?;
            LibraryImport {
                folders: vec![],
                phrases,
            }
        }
        serde_json::Value::Object(map)
            if map.contains_key("folders") || map.contains_key("phrases") =>
        {
            serde_json::from_value::<LibraryImport>(value.clone()).map_err(|e| {
                ApiError::from(DomainError::Validation(format!(
                    "invalid library JSON: {e}"
                )))
            })?
        }
        serde_json::Value::Object(_) => {
            // Assume aText backup format (numeric keys: {"4":[groups...]}).
            let (folders, phrases) = parse_atext_library(&value)?;
            LibraryImport { folders, phrases }
        }
        _ => {
            return Err(ApiError::from(DomainError::Validation(
                "JSON must be an object or array".into(),
            )));
        }
    };

    // Folders: create with remapped parent_ids, rooted at `folder_id` (or root). Topologically
    // create parents before children; unresolved parents (cycles) fall back to root.
    let mut folder_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut pending = lib.folders;
    let mut passes = 0;
    while !pending.is_empty() && passes < 16 {
        let mut deferred = Vec::new();
        for f in pending.drain(..) {
            let parent_new = match &f.parent_id {
                Some(p) => match folder_map.get(p) {
                    Some(n) => Some(n.clone()),
                    None => {
                        deferred.push(f);
                        continue;
                    }
                },
                None => folder_id.clone(),
            };
            let new_id = Uuid::new_v4().to_string();
            let inserted = state
                .db
                .create_folder(&Folder {
                    id: new_id.clone(),
                    name: f.name,
                    parent_id: parent_new,
                    created_at: now_iso(),
                    enabled: f.enabled,
                })
                .is_ok();
            folder_map.insert(
                f.id,
                if inserted {
                    new_id
                } else {
                    folder_id.clone().unwrap_or_default()
                },
            );
        }
        pending = deferred;
        passes += 1;
    }
    for f in pending {
        let new_id = Uuid::new_v4().to_string();
        let _ = state.db.create_folder(&Folder {
            id: new_id.clone(),
            name: f.name,
            parent_id: folder_id.clone(),
            created_at: now_iso(),
            enabled: f.enabled,
        });
        folder_map.insert(f.id, new_id);
    }

    // Phrases: pre-assign fresh IDs so Nested-macro phrase_id refs can be remapped.
    let phrase_id_map: std::collections::HashMap<String, String> = lib
        .phrases
        .iter()
        .map(|p| (p.id.clone(), Uuid::new_v4().to_string()))
        .collect();

    let mut count = 0u32;
    for mut p in lib.phrases {
        // Trust boundary: an imported library must not carry Shell/File/Env macros
        // (every other import format already only produces inert text phrases).
        if p.has_untrusted_macros() {
            crate::debug_log(&format!("import: skipping phrase with Shell/File/Env macro: {}", p.name));
            continue;
        }
        // Remap Nested-macro references to the imported phrase's new id.
        for seg in p.body.iter_mut() {
            if let Segment::Macro {
                macro_value: Macro::Nested { phrase_id },
            } = seg
            {
                if let Some(new_pid) = phrase_id_map.get(phrase_id) {
                    *phrase_id = new_pid.clone();
                }
            }
        }
        // Resolve a home folder: the remapped original, else the target, else skip.
        let folder = if let Some(f) = folder_map.get(&p.folder_id).filter(|f| !f.is_empty()) {
            f.clone()
        } else if let Some(t) = &folder_id {
            t.clone()
        } else {
            continue;
        };
        p.id = phrase_id_map
            .get(&p.id)
            .cloned()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        p.folder_id = folder;
        p.created_at = now_iso();
        p.updated_at = now_iso();
        p.deleted_at = None;
        if state.db.create_phrase(&p).is_ok() {
            count += 1;
        }
    }

    after_library_change(&state, None, false);
    Ok(count)
}

/// Resolve or create a folder for imports.
fn resolve_folder(
    state: &State<'_, AppState>,
    folder_id: Option<String>,
) -> Result<String, ApiError> {
    Ok(match folder_id {
        Some(f) => f,
        None => {
            state
                .db
                .create_folder(&Folder {
                    id: Uuid::new_v4().to_string(),
                    name: "Imported".into(),
                    parent_id: None,
                    created_at: now_iso(),
                    enabled: true,
                })?
                .id
        }
    })
}

/// Build a single text-segment phrase with an optional autotext trigger — the shape
/// every plain import path (CSV, aText, JSON-fallback) produces.
fn simple_phrase(folder_id: String, abbr: &str, text: &str, name: String) -> Phrase {
    let triggers = if abbr.is_empty() {
        Vec::new()
    } else {
        vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some(abbr.to_string()),
            key: None,
            regex: false,
            execute: Default::default(),
        }]
    };
    Phrase {
        id: Uuid::new_v4().to_string(),
        folder_id,
        name,
        abbreviation: if abbr.is_empty() {
            None
        } else {
            Some(abbr.to_string())
        },
        content_type: ContentType::Plain,
        body: vec![Segment::Text {
            text: text.to_string(),
        }],
        triggers,
        sensitive: false,
        app_scope: None,
        expanded_count: 0,
        last_used_at: None,
        case_mode: Default::default(),
        created_at: now_iso(),
        updated_at: now_iso(),
        deleted_at: None,
        tags: Vec::new(),
    }
}

/// Find phrases with duplicate first-text content, grouped.
#[tauri::command]
pub fn find_duplicates(state: State<'_, AppState>) -> Result<Vec<Vec<PhraseSummary>>, ApiError> {
    let groups = state.db.find_duplicates()?;
    Ok(groups
        .iter()
        .map(|g| g.iter().map(|p| state.db.summary(p)).collect())
        .collect())
}

/// Version history for a phrase.
#[tauri::command]
pub fn list_versions(
    phrase_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<PhraseVersion>, ApiError> {
    Ok(state.db.list_versions(&phrase_id)?)
}

/// Restore a phrase to a prior version (snapshots the current state first).
#[tauri::command]
pub fn restore_version(
    phrase_id: String,
    version_id: String,
    state: State<'_, AppState>,
) -> Result<Phrase, ApiError> {
    let p = state.db.restore_version(&phrase_id, &version_id)?;
    after_library_change(&state, None, false);
    Ok(p)
}

// -- Autotext -------------------------------------------------------------- //
#[derive(Debug, serde::Serialize)]
pub struct AutotextStatus {
    pub enabled: bool,
    pub running: bool,
    pub events_seen: u64,
    pub abbreviations: usize,
    /// macOS only: the Accessibility grant (None elsewhere). False means the listener
    /// runs but macOS silently drops every event.
    pub accessibility_granted: Option<bool>,
    /// macOS only: Secure Input is suppressing global taps (password field focused).
    pub secure_input_active: Option<bool>,
}

#[tauri::command]
pub fn autotext_status(state: State<'_, AppState>) -> AutotextStatus {
    let perms = crate::platform::permissions();
    AutotextStatus {
        enabled: state.autotext_enabled.load(Ordering::Relaxed),
        running: state.autotext_running.load(Ordering::Relaxed),
        events_seen: state.autotext_events.load(Ordering::Relaxed),
        abbreviations: state.abbrev_index.lock().map(|i| i.len()).unwrap_or(0),
        accessibility_granted: perms.accessibility_granted,
        secure_input_active: perms.secure_input_active,
    }
}

/// Offer-mode: apply a chosen correction. Validates `corrected` was an offered candidate,
/// then echo-suppresses `backspace(orig + space) + type(corrected + space)` into the
/// re-focused text app. Returns false (no-op) if no valid pending offer exists.
#[tauri::command]
pub fn apply_correction(
    corrected: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, ApiError> {
    let Some(orig) = crate::triggers::autotext::take_pending_offer(
        &corrected,
        &state.spellcheck_offer,
        &state.spellcheck_offer_candidates,
    ) else {
        return Ok(false);
    };
    clear_spellcheck_offer(&state);
    // Learn-on-accept: the user chose this correction, so persist the typo→correction
    // pair. Next occurrence corrects via exact-match (no popup, no re-ranking). `orig`
    // may carry punctuation wrappers ("(teh,") — learn the clean core so future
    // occurrences match regardless of wrappers.
    let core_pair = crate::triggers::autotext::learn_pair_from_offer(&orig, &corrected);
    if let Some((typo, fixed)) = core_pair {
        if let Ok(mut g) = state.spellcheck_learned.lock() {
            g.insert(typo.clone(), fixed.clone());
        }
        let _ = state.db.add_spellcheck_learned(&typo, &fixed);
    }
    crate::triggers::autotext::inject_with_suppression(
        &app,
        &state.autotext_injecting,
        &state.autotext_last_echo,
        move |inj| {
            crate::triggers::autotext::inject_correction(&orig, &corrected, inj);
        },
    );
    Ok(true)
}

/// Offer-mode "delete typed word" (Backspace while the popup is open): erase the offered word
/// and its trailing space from the field. Unlike `apply_correction` there's no candidate to
/// validate, so this does NOT go through `take_pending_offer` — the backspace count is derived
/// from the stored RAW offer token itself (set by the listener), so a rogue frontend can't drive
/// an arbitrary `backspace(N)` count.
#[tauri::command]
pub fn delete_offer_word(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, ApiError> {
    let Some(orig) = state.spellcheck_offer.lock().ok().and_then(|g| g.clone()) else {
        return Ok(false);
    };
    clear_spellcheck_offer(&state);
    crate::triggers::autotext::inject_with_suppression(
        &app,
        &state.autotext_injecting,
        &state.autotext_last_echo,
        move |inj| {
            crate::triggers::autotext::inject_delete_word(&orig, inj);
        },
    );
    Ok(true)
}

/// Clear the pending spellcheck offer + ask the listener to drop its now-stale
/// thread-local buffer.
fn clear_spellcheck_offer(state: &State<'_, AppState>) {
    if let Ok(mut g) = state.spellcheck_offer.lock() {
        *g = None;
    }
    if let Ok(mut g) = state.spellcheck_offer_candidates.lock() {
        *g = Vec::new();
    }
    state.spellcheck_buffer_clear.store(true, Ordering::SeqCst);
}

/// Offer-mode "Add to dictionary": persist the word to the ignore list so it won't be
/// offered again, and update the runtime set immediately.
#[tauri::command]
pub fn spellcheck_ignore_word(
    word: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), ApiError> {
    use tauri::Emitter;
    let w = word.trim().to_lowercase();
    if w.is_empty() {
        return Ok(());
    }
    // Runtime set update must happen regardless of persistence outcome.
    if let Ok(mut guard) = state.spellcheck_ignore.lock() {
        guard.insert(w.clone());
    }
    let _ = state.db.add_spellcheck_ignore(&w);
    let _ = app.emit("spellcheck-learned", w);
    Ok(())
}

/// Run `f` on the GTK main thread and block until it completes. Raw X11 calls share the X
/// display connection with GTK (which does its own X access on the main thread), so they must
/// be serialized there or they race / silently no-op.
pub(crate) fn on_main<R: Send + 'static>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&tauri::AppHandle) -> R + Send + 'static,
) -> Option<R> {
    let (tx, rx) = std::sync::mpsc::channel();
    let app2 = app.clone();
    // If scheduling fails, `tx` drops with the closure and rx.recv() returns Err → None.
    let _ = app.run_on_main_thread(move || {
        let _ = tx.send(f(&app2));
    });
    rx.recv().ok()
}

/// Force keyboard focus onto the popup window. Called by the frontend after the offer/chooser
/// view has rendered (once the window is fully mapped) — this is the reliable moment to grab
/// focus, since the early grab in `show_spellcheck_offer` can race the window being mapped.
/// On X11 it bypasses WM focus-stealing prevention via a direct `XSetInputFocus`, on the main
/// thread so it doesn't race GTK.
#[tauri::command]
pub fn focus_popup(app: tauri::AppHandle) {
    on_main(&app, |a| {
        use tauri::Manager;
        if let Some(win) = a.get_webview_window("popup") {
            #[cfg(target_os = "windows")]
            crate::triggers::autotext::force_focus_win();
            let _ = win.set_focus();
            crate::triggers::autotext::force_focus_x11(&win);
        }
    });
}

/// Size the popup to its content (the frontend measures its mode view and calls
/// this after render). Both dims are optional and clamped, so a runaway webview
/// can't create an unusable window; width stays authoritative from the caller —
/// the webview mirrors the viewport width, so it naturally round-trips unchanged.
#[tauri::command]
pub fn resize_popup(app: tauri::AppHandle, width: Option<f64>, height: Option<f64>) {
    let Some(win) = app.get_webview_window("popup") else {
        return;
    };
    let w = width.map(|v| v.clamp(280.0, 900.0));
    let h = height.map(|v| v.clamp(120.0, 720.0));
    if let (Some(w), Some(h)) = (w, h) {
        let _ = win.set_size(Size::Logical(LogicalSize::new(w, h)));
    }
}

/// Take an active X11 keyboard grab on the popup so all keystrokes route to it (independent of
/// window focus) until `release_keyboard` or the window hides. Retries because `XGrabKeyboard`
/// requires the window to be viewable, and mapping can lag `show()` by a few ms.
#[tauri::command]
pub fn grab_keyboard(app: tauri::AppHandle) {
    for attempt in 1..=12 {
        let ok = on_main(&app, |a| {
            use tauri::Manager;
            a.get_webview_window("popup")
                .map(|win| crate::triggers::autotext::grab_keyboard_x11(&win))
                .unwrap_or(false)
        })
        .unwrap_or(false);
        if ok {
            eprintln!("[phrasexp] keyboard grab succeeded on attempt {attempt}");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    eprintln!("[phrasexp] keyboard grab FAILED after retries (window not viewable?)");
}

/// Release the keyboard grab taken by `grab_keyboard`. Called on every offer dismiss path
/// (accept / ignore / Esc) before hiding — also belt-and-suspenders for the auto-release X does
/// when the window unmaps.
#[tauri::command]
pub fn release_keyboard(app: tauri::AppHandle) {
    // Stop routing nav keys to the (now-dismissed) offer popup.
    crate::triggers::autotext::offer_latch_off();
    on_main(&app, |a| {
        use tauri::Manager;
        if let Some(win) = a.get_webview_window("popup") {
            crate::triggers::autotext::ungrab_keyboard_x11(&win);
        }
        // macOS: the popup took real key focus (no grab exists), so PhraseXp is the
        // active app. Hiding it hands activation back to the previous (typing) app —
        // a plain win.hide() would strand focus here. Skip when the main window is
        // open (user is interacting with the app itself).
        #[cfg(target_os = "macos")]
        {
            let main_visible = a
                .get_webview_window("main")
                .map(|w| w.is_visible().unwrap_or(false))
                .unwrap_or(false);
            if !main_visible {
                let _ = a.hide();
            }
        }
    });
}

// -- Settings -------------------------------------------------------------- //
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, ApiError> {
    Ok(state.db.load_settings()?)
}

/// Report whether at-rest encryption is actually active. `enabled` is the user's
/// intent (the setting); `available` is whether a DEK is loaded (keychain present). Encryption
/// is only in effect when both are true — the UI uses this to warn when the toggle can't work.
#[tauri::command]
pub fn crypto_status(state: State<'_, AppState>) -> Result<serde_json::Value, ApiError> {
    let enabled = state
        .db
        .load_settings()
        .map(|s| s.encrypt_at_rest)
        .unwrap_or(false);
    Ok(serde_json::json!({
        "enabled": enabled,
        "available": state.db.crypto_available(),
    }))
}

#[tauri::command]
pub fn set_settings(
    payload: Settings,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Settings, ApiError> {
    // Detect an encryption-at-rest toggle so we can re-migrate the whole library
    // (encrypt-all on enable, decrypt-all on disable) and avoid a mixed plaintext/cipher state.
    let prev_encrypt_at_rest = state.db.load_settings().ok().map(|s| s.encrypt_at_rest);
    let saved = state.db.save_settings(&payload)?;

    // Apply runtime side-effects.
    state
        .autotext_enabled
        .store(saved.autotext_enabled, Ordering::Relaxed);
    state
        .clipboard_enabled
        .store(saved.clipboard_enabled, Ordering::Relaxed);
    state
        .clipboard_max_length
        .store(saved.clipboard_max_length, Ordering::Relaxed);
    state
        .clipboard_max_entries
        .store(saved.clipboard_max_entries, Ordering::Relaxed);
    state
        .spellcheck_enabled
        .store(saved.spellcheck_enabled, Ordering::Relaxed);
    state
        .case_adapt_enabled
        .store(saved.case_adapt_enabled, Ordering::Relaxed);
    state
        .review_sentence
        .store(saved.review_sentence, Ordering::Relaxed);
    if let Ok(mut guard) = state.spellcheck_ignore.lock() {
        *guard = csv_set(&saved.spellcheck_ignore);
    }
    if let Ok(mut guard) = state.spellcheck_mode.lock() {
        *guard = saved.spellcheck_mode.clone();
    }
    if let Ok(mut guard) = state.spellcheck_delivery.lock() {
        *guard = saved.spellcheck_delivery.clone();
    }
    // Spellcheck toggle hotkey: the rdev listener reads this live from the shared
    // mutex, so updating it here takes effect on the next keypress — no shortcut
    // re-registration needed (reregister_hotkeys only handles popup/per-phrase).
    if let Ok(mut guard) = state.spellcheck_hotkey.lock() {
        *guard = saved.spellcheck_hotkey.clone();
    }
    if let Ok(mut guard) = state.spellcheck_exclude_apps.lock() {
        *guard = csv_set(&saved.spellcheck_exclude_apps);
    }
    if let Ok(mut guard) = state.autotext_exclude_apps.lock() {
        *guard = csv_set(&saved.autotext_exclude_apps);
    }
    // Settings changed — the poller may no longer (or now) need the window title.
    state.refresh_app_poll_needed();

    // Re-register popup + per-phrase hotkeys (cheap; covers popup-hotkey changes).
    crate::reregister_hotkeys(&app);

    // If the at-rest encryption toggle changed, re-migrate the library in one pass so
    // there's no mixed plaintext/cipher state. No-op when the keychain is unavailable.
    if prev_encrypt_at_rest != Some(saved.encrypt_at_rest) && state.db.crypto_available() {
        if saved.encrypt_at_rest {
            let _ = state.db.migrate_encrypt_at_rest();
        } else {
            let _ = state.db.decrypt_at_rest();
        }
    }

    // Launch-on-login.
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let _ = if saved.autostart {
        al.enable()
    } else {
        al.disable()
    };

    Ok(saved)
}

// -- helpers --------------------------------------------------------------- //
fn apply_update(existing: Phrase, u: PhraseUpdate) -> DomainResult<Phrase> {
    let mut abbreviation = u.abbreviation.clone().or(existing.abbreviation.clone());
    let triggers = u
        .triggers
        .clone()
        .unwrap_or_else(|| existing.triggers.clone());
    if let Some(t) = canonicalize_autotext(&triggers)? {
        abbreviation = t.abbreviation.clone();
    }
    let updated = Phrase {
        id: existing.id,
        folder_id: u.folder_id.unwrap_or(existing.folder_id),
        name: u.name.unwrap_or(existing.name),
        abbreviation,
        content_type: u.content_type.unwrap_or(existing.content_type),
        body: u.body.unwrap_or(existing.body),
        triggers,
        sensitive: u.sensitive.unwrap_or(existing.sensitive),
        app_scope: u.app_scope.or(existing.app_scope),
        // Usage stats are never part of an update payload — always carried over.
        expanded_count: existing.expanded_count,
        last_used_at: existing.last_used_at.clone(),
        case_mode: u.case_mode.unwrap_or(existing.case_mode),
        created_at: existing.created_at,
        updated_at: existing.updated_at,
        deleted_at: existing.deleted_at,
        tags: u.tags.unwrap_or(existing.tags),
    };
    // Same validation the create path enforces (body non-empty, content type allowed).
    updated.validate()?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn substitute_template_replaces_param_and_selection() {
        let one = |t: &str| HashMap::from([("language".into(), t.to_string())]);
        let out = substitute_template("To {language}:\n{clipboard}", "hello", &one("German"));
        assert_eq!(out, "To German:\nhello");
        // No params → the token is left literal (required_params guards this path).
        let out = substitute_template("{length} summary of {selection}", "text", &HashMap::new());
        assert_eq!(out, "{length} summary of text");
        // Braces that aren't tokens (spaces/hyphens) are preserved.
        let some_x = HashMap::from([("x".into(), "x".to_string())]);
        let out = substitute_template("keep {not a param} and {x-y} here", "s", &some_x);
        assert_eq!(out, "keep {not a param} and {x-y} here");
        // Every occurrence of the param token is replaced.
        let out = substitute_template("{language} text, to {language}", "s", &one("German"));
        assert_eq!(out, "German text, to German");
        // Substitutable-but-unanswered tokens stay literal everywhere.
        let out = substitute_template("{tone} and {tone}", "s", &HashMap::new());
        assert_eq!(out, "{tone} and {tone}");
    }

    #[test]
    fn param_tokens_lists_every_distinct_interactive_token() {
        assert_eq!(
            param_tokens("Summarize — {length}, as {format}:\n{clipboard}"),
            vec!["length".to_string(), "format".to_string()]
        );
        // Duplicates collapse, clipboard/selection never appear.
        assert_eq!(
            param_tokens("{a} {a} {clipboard} {selection} {b}"),
            vec!["a", "b"]
        );
        assert_eq!(
            param_tokens("Fix grammar:\n{clipboard}"),
            Vec::<String>::new()
        );
        // Unclosed brace stops the scan.
        assert_eq!(param_tokens("x {ok} and {"), vec!["ok".to_string()]);
    }

    #[test]
    fn substitute_template_handles_multiple_tokens() {
        let params = HashMap::from([
            ("length".into(), "30 words".to_string()),
            ("format".into(), "bullet points".to_string()),
        ]);
        let out = substitute_template(
            "Summarize in {length} as {format}:\n{clipboard}",
            "long text",
            &params,
        );
        assert_eq!(out, "Summarize in 30 words as bullet points:\nlong text");
        // One answered, one not: the answered one substitutes, the other stays literal.
        let half = HashMap::from([("length".into(), "30 words".to_string())]);
        let out = substitute_template("{length} as {format}", "s", &half);
        assert_eq!(out, "30 words as {format}");
    }

    #[test]
    fn required_params_enforce_every_placeholder() {
        assert!(required_params("Summarize {length}:", None).is_err());
        let blank = HashMap::from([("length".into(), "  ".to_string())]);
        assert!(required_params("Summarize {length}:", Some(&blank)).is_err());
        let ok = HashMap::from([("length".into(), "30 words".to_string())]);
        assert_eq!(
            required_params("Summarize {length}:", Some(&ok)).unwrap(),
            ok
        );
        // Multi-token: all must be answered; error names the first missing one.
        assert!(required_params("{a} and {b}", Some(&ok)).is_err());
        let both = HashMap::from([
            ("a".into(), "1".to_string()),
            ("b".into(), "2 ".to_string()),
        ]);
        assert_eq!(
            required_params("{a} and {b}", Some(&both)).unwrap()["b"],
            "2"
        );
        // No placeholder → empty map passes.
        assert!(required_params("Fix:\n{clipboard}", None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn parses_json_with_bom_and_trailing_nul() {
        // aText-style: UTF-8 BOM + JSON + trailing NUL byte. from_str would reject both.
        let raw = "\u{feff}{\"4\":[]}\u{0000}";
        let v = parse_first_json_value(raw).expect("should parse leniently");
        assert!(v.get("4").is_some(), "root object parsed");
        // Trailing garbage (non-NUL) is also ignored.
        let v2 = parse_first_json_value("{\"a\":1}!!!garbage").unwrap();
        assert_eq!(v2.get("a").and_then(|x| x.as_i64()), Some(1));
    }

    #[test]
    fn parses_atext_backup_into_folders_and_phrases() {
        // Mirrors aText's numeric-key schema (BOM + trailing NUL included, like real exports).
        let raw = "\u{feff}{\"0\":\"root\",\"4\":[\
            {\"2\":\"Coding\",\"13\":[\
                {\"1\":[\"py-venv/\"],\"3\":\"t\",\"4\":\"python -m venv .venv\",\"12\":\"venv helper\"},\
                {\"1\":[\"clip/\"],\"3\":\"t\",\"4\":\"see <atext contenteditable=\\\"false\\\">X</atext> here\"}\
            ]}\
        ]}\u{0000}";
        let value = parse_first_json_value(raw).unwrap();
        let (folders, phrases) = parse_atext_library(&value).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Coding");
        assert_eq!(phrases.len(), 2);
        // first snippet: abbr + content + description as name
        assert_eq!(phrases[0].abbreviation.as_deref(), Some("py-venv/"));
        assert_eq!(phrases[0].name, "venv helper");
        assert!(phrases[0].body.iter().any(|s| matches!(s, crate::domain::models::Segment::Text { text } if text == "python -m venv .venv")));
        // second snippet: aText <atext> markers stripped, content used as name
        assert_eq!(phrases[1].abbreviation.as_deref(), Some("clip/"));
        assert!(phrases[1].name.starts_with("see "));
        assert!(!phrases[1].name.contains("<atext"));
        assert!(!phrases[1].name.contains("</atext>"));
        // both phrases link to the imported folder (synthetic id remapped later by import_json)
        assert_eq!(phrases[0].folder_id, folders[0].id);
        assert_eq!(phrases[1].folder_id, folders[0].id);
    }

    #[test]
    fn parse_variations_splits_and_trims() {
        let raw = "First one.\n---\n  Second one.  \n---\n---\nThird one.\n---\nFourth";
        assert_eq!(
            parse_variations(raw, 5),
            vec!["First one.", "Second one.", "Third one.", "Fourth"]
        );
        // count caps the result.
        assert_eq!(parse_variations(raw, 2), vec!["First one.", "Second one."]);
        // No separators → whole response is one variation (still usable).
        assert_eq!(
            parse_variations("Just one variation.", 3),
            vec!["Just one variation."]
        );
        // Empty response → empty vec → command errors.
        assert!(parse_variations("---\n---", 3).is_empty());
    }

    // -- TextExpander / Espanso import parsers (1.6) -------------------------- //
    #[test]
    fn textexpander_csv_header_and_positional() {
        // Official format: abbreviation,snippet,label with header row.
        let official = "abbreviation,snippet,label\n sig,Hello world, Greeting\ninv,\"Line one\nLine two\",Invoice\n";
        let rows = parse_textexpander_csv(official);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            ("sig".into(), "Hello world".into(), Some("Greeting".into()))
        );
        assert_eq!(rows[1].1, "Line one\nLine two"); // quoted multi-line survives
        assert_eq!(rows[1].2.as_deref(), Some("Invoice"));

        // Headerless fallback: positional (abbr, content), label absent.
        let plain = "br,Best regards\n";
        let rows = parse_textexpander_csv(plain);
        assert_eq!(rows[0], ("br".into(), "Best regards".into(), None));

        // Empty abbr or body rows are skipped.
        let junk = "abbreviation,snippet,label\n,notext,label\nabbr,\n";
        assert!(parse_textexpander_csv(junk).is_empty());
    }

    #[test]
    fn espanso_yaml_simple_multiline_and_quoted() {
        let yaml = concat!(
            "matches:\n",
            "  - trigger: \":hi\"\n",
            "    replace: \"Hi there!\"\n",
            "  - trigger: :sig\n",
            "    replace: |\n",
            "      Best regards,\n",
            "      Sam\n",
            "  - trigger: :q\n",
            "    replace: >-\n",
            "      folded text\n",
        );
        let rows = parse_espanso_yaml(yaml);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], (":hi".into(), "Hi there!".into(), None));
        assert_eq!(rows[1].1, "Best regards,\nSam");
        assert_eq!(rows[2].1, "folded text");
    }

    #[test]
    fn espanso_yaml_ignores_forms_vars_and_scripts() {
        // A match with `form:` instead of `replace:` is skipped; script/clipboard
        // vars are inert (imported text only, never interpreted). Anything before
        // `matches:` (config keys) is ignored.
        let yaml = concat!(
            "config_option: true\n",
            "matches:\n",
            "  - trigger: :form\n",
            "    form: |\"Name?\"]\n",
            "  - trigger: :date\n",
            "    replace: '{{mydate}}'\n",
            "    vars:\n",
            "      - name: mydate\n",
            "        type: date\n",
            "        params: {format: \"%Y\"}\n",
            "  - trigger: :run\n",
            "    replace: '{{output}}'\n",
            "    vars:\n",
            "      - name: output\n",
            "        type: script\n",
            "        params: {args: [\"rm -rf /\"]}\n",
        );
        let rows = parse_espanso_yaml(yaml);
        // The form match is gone; date/script matches import as LITERAL text — the
        // var references and the script payload never execute in this app.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (":date".into(), "{{mydate}}".into(), None));
        assert_eq!(rows[1], (":run".into(), "{{output}}".into(), None));
        assert!(!rows.iter().any(|(_, r, _)| r.contains("rm -rf")));
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use crate::domain::models::{ContentType, Segment, TriggerBinding, TriggerType};

    fn phrase(abbr: Option<&str>, body: Vec<Segment>, regex: bool) -> Phrase {
        Phrase {
            id: "p".into(),
            folder_id: "f".into(),
            name: "Phrase".into(),
            abbreviation: abbr.map(String::from),
            content_type: ContentType::Plain,
            body,
            triggers: vec![TriggerBinding {
                kind: TriggerType::Autotext,
                abbreviation: abbr.map(String::from),
                key: None,
                regex,
                execute: Default::default(),
            }],
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: String::new(),
            updated_at: String::new(),
            deleted_at: None,
            tags: Vec::new(),
        }
    }

    fn text(s: &str) -> Vec<Segment> {
        vec![Segment::Text { text: s.into() }]
    }

    #[test]
    fn export_rows_keep_text_only() {
        let ps = vec![
            phrase(Some("sig"), text("Best regards"), false),
            // macro body → skipped
            phrase(
                Some("dt"),
                vec![Segment::Macro {
                    macro_value: crate::domain::models::Macro::Uuid,
                }],
                false,
            ),
            // regex trigger → skipped
            phrase(Some(":[a-z]+"), text("x"), true),
            // no abbreviation → skipped
            phrase(None, text("y"), false),
        ];
        let (rows, skipped) = export_rows(&ps);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trigger, "sig");
        assert_eq!(rows[0].text, "Best regards");
        assert_eq!(skipped, 3);
    }

    #[test]
    fn textexpander_csv_round_trips_commas_and_newlines() {
        let rows = vec![ExportRow {
            trigger: ":addr".into(),
            label: "Address".into(),
            text: "Line1, with comma\nLine2".into(),
        }];
        let csv = textexpander_csv(&rows);
        assert!(csv.starts_with("abbreviation,snippet,label"), "header: {csv}");
        // The csv writer quotes fields containing commas/newlines — parse it back.
        let parsed = parse_textexpander_csv(&csv);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, ":addr");
        assert_eq!(parsed[0].1, "Line1, with comma\nLine2");
    }

    #[test]
    fn plain_csv_round_trips() {
        let rows = vec![ExportRow {
            trigger: "a1".into(),
            label: "n".into(),
            text: "hello, world".into(),
        }];
        let csv = plain_csv(&rows);
        assert_eq!(csv, "a1,\"hello, world\"\n");
    }

    #[test]
    fn espanso_yaml_has_matches_shape() {
        let rows = vec![ExportRow {
            trigger: ":hello".into(),
            label: "Greeting".into(),
            text: "multi\nline".into(),
        }];
        let yaml = espanso_yaml(&rows);
        assert!(yaml.starts_with("matches:"), "shape: {yaml}");
        assert!(yaml.contains("trigger: ':hello'") || yaml.contains("trigger: \":hello\"") || yaml.contains("trigger: :hello"), "trigger: {yaml}");
        assert!(yaml.contains("multi"), "multiline replace missing: {yaml}");
    }
}
