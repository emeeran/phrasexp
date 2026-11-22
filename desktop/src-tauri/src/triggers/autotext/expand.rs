//! Expansion engine: delayed (delimiter-mode) pending matches, case adaptation,
//! phrase expansion + injection (`maybe_expand` / `fire_delayed`), and the shared
//! echo-quiet wait.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

use crate::db::Db;
use crate::domain::case_adapt;
use crate::domain::models::{CaseMode, Macro, Phrase, Segment};
use crate::triggers::Injector;

use super::index::AbbrevIndex;
use super::offer::show_chooser;

/// Does the phrase body contain a macro matching `f`?
fn has_macro(p: &Phrase, f: impl Fn(&Macro) -> bool) -> bool {
    p.body
        .iter()
        .any(|s| matches!(s, Segment::Macro { macro_value } if f(macro_value)))
}

/// Does the phrase need the inputform popup before it can expand?
fn has_inputform(p: &Phrase) -> bool {
    has_macro(p, |m| matches!(m, Macro::InputForm { .. }))
}

/// Does the phrase contain an AI macro (which needs a blocking network call)?
fn has_ai(p: &Phrase) -> bool {
    has_macro(p, |m| {
        matches!(m, Macro::Ai { .. } | Macro::AiAction { .. })
    })
}

// ------------------------------------------------------------------------- //
// 1.4 delimiter-mode pending match (pure state transitions, unit-tested)
// ------------------------------------------------------------------------- //
/// A matched delayed abbreviation parked until the next key decides its fate.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingMatch {
    /// The abbreviation text as typed (what must still end the buffer to fire).
    pub(super) abbr: String,
    /// Candidate phrase ids (may be >1 → chooser on fire, like immediate mode).
    pub(super) ids: Vec<String>,
}

/// What the listener should do with a pending match for a given next key.
/// (Return/Tab are intercepted earlier, before `key_to_char`, so this helper
/// only ever deals with printable chars.)
#[derive(Debug, PartialEq)]
pub(super) enum PendingAction {
    /// Fire the expansion; the delimiter char is re-typed after it.
    Fire(char),
    /// The pending match is dead — drop it and process the key normally.
    Drop,
}

/// The fixed delimiter set: space + common punctuation. Enter/Tab aren't
/// `key_to_char` outputs, so the listener matches them as raw keys
/// (see the `Key::Return | Key::Tab` arm).
fn is_delimiter_char(c: char) -> bool {
    matches!(c, ' ' | ',' | '.' | ';' | ':' | '!' | '?')
}

/// Pure decision for a printable char arriving while a pending match exists.
/// `buffer` is the buffer BEFORE the char is pushed.
pub(super) fn transition_pending(pending: &PendingMatch, ch: char, buffer: &str) -> PendingAction {
    if is_delimiter_char(ch) && buffer.ends_with(&pending.abbr) {
        PendingAction::Fire(ch)
    } else {
        PendingAction::Drop
    }
}

/// Apply case adaptation to every text chunk of a chunk-stream expansion — same
/// transform as the flat view, applied chunk-wise (the transform depends only on
/// the abbreviation, never on surrounding text).
fn apply_case_mode_chunks(
    mut chunks: Vec<crate::domain::models::OutChunk>,
    abbr: &str,
    mode: CaseMode,
    global_on: bool,
) -> Vec<crate::domain::models::OutChunk> {
    for c in &mut chunks {
        if let crate::domain::models::OutChunk::Text(t) = c {
            *t = apply_case_mode(std::mem::take(t), abbr, mode, global_on);
        }
    }
    chunks
}

/// Case adaptation honoring the per-phrase override (1.4):
/// Always → adapt regardless of the global toggle; Never → verbatim;
/// Adapt → follow the global toggle (the previous, default behavior).
fn apply_case_mode(output: String, abbr: &str, mode: CaseMode, global_on: bool) -> String {
    match mode {
        CaseMode::Always => case_adapt(&output, abbr),
        CaseMode::Never => output,
        CaseMode::Adapt => {
            if global_on {
                case_adapt(&output, abbr)
            } else {
                output
            }
        }
    }
}

/// Erase `n` characters with echo suppression on — injected backspaces would
/// otherwise re-enter this listener's buffer (see the inject-on-worker-thread
/// note in `maybe_expand`).
fn erase_suppressed(inj: &dyn Injector, inj_flag: &Arc<AtomicBool>, n: usize) {
    inj_flag.store(true, Ordering::SeqCst);
    crate::triggers::inject::log_if_err(inj.backspace(n), "erase abbreviation");
    inj_flag.store(false, Ordering::SeqCst);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_expand(
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    db: &Db,
    inj: &dyn Injector,
    inj_flag: &Arc<AtomicBool>,
    case_adapt_flag: &AtomicBool,
    current: &Mutex<Option<String>>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
    app: &tauri::AppHandle,
) {
    let matched = idx.lock().map(|i| i.match_suffix(buffer)).ok().flatten();
    let Some((abbr, ids)) = matched else {
        // Diagnostic: what the buffer holds when a word completes, so a mismatch between
        // typed keys and indexed abbreviations is visible instead of silent.
        if buffer.ends_with([' ', '\n', '\t']) {
            crate::debug_log(&format!("word done, no match: {buffer:?}"));
        }
        return;
    };
    crate::debug_log(&format!("match {abbr:?} → {} phrase(s)", ids.len()));

    if ids.len() > 1 {
        // Multi-match: erase the typed abbreviation and surface a chooser.
        erase_suppressed(inj, inj_flag, abbr.chars().count());
        buffer.clear();
        show_chooser(app, ids);
        return;
    }

    let id = &ids[0];
    // Cache-first: the index holds parsed phrases from rebuild time, so the hot
    // path skips the per-expansion DB read + JSON parse/decrypt entirely. Misses
    // (sensitive phrases, freshly-edited rows) fall back to the DB.
    let phrase: std::sync::Arc<Phrase> = {
        let cached = idx.lock().ok().and_then(|i| i.cached_phrase(id));
        match cached {
            Some(p) => p,
            None => match db.get_phrase(id) {
                Ok(Some(p)) => std::sync::Arc::new(p),
                _ => return,
            },
        }
    };
    if phrase.deleted_at.is_some() || has_inputform(&phrase) {
        return; // inputform phrases need the popup; skip silent expansion.
    }
    // App scoping — only fire for this phrase if its app_scope matches the focused window.
    if let Some(scope) = &phrase.app_scope {
        let cur = current.lock().map(|g| g.clone()).ok().flatten();
        if !crate::triggers::app_context::scope_active(scope, &cur) {
            return; // wrong application focused
        }
    }

    // AI macros make blocking network calls (1-3s). Erase the abbreviation now
    // and expand + inject in a background thread so the listener isn't blocked.
    if has_ai(&phrase) {
        erase_suppressed(inj, inj_flag, abbr.chars().count());
        buffer.clear();

        let state = app.state::<crate::AppState>();
        let db_arc = state.db.clone();
        let inj_arc = state.injector.clone();
        let phrase_owned = phrase;
        let app_clone = app.clone();
        let case_adapt_on = case_adapt_flag.load(Ordering::Relaxed);
        let case_mode = phrase_owned.case_mode;
        let abbr_owned = abbr.clone();
        std::thread::spawn(move || {
            match tauri::async_runtime::block_on(db_arc.expand_chunks_with_clip(
                &phrase_owned,
                &HashMap::new(),
                None,
            )) {
                Ok(chunks) => {
                    let chunks =
                        apply_case_mode_chunks(chunks, &abbr_owned, case_mode, case_adapt_on);
                    let _ = crate::triggers::inject::deliver_chunks(
                        &*inj_arc,
                        phrase_owned.content_type,
                        &chunks,
                    );
                    // Usage stats (1.4): count the expansion once it's delivered.
                    let _ = db_arc.record_usage(&phrase_owned.id);
                }
                Err(e) => {
                    eprintln!("[phrasexp] AI expansion error: {e}");
                    let _ = app_clone.emit("phrase-error", format!("{e}"));
                }
            }
        });
        return;
    }

    // Non-AI: expand synchronously (fast — pure computation, no network).
    let chunks = match tauri::async_runtime::block_on(db.expand_chunks_with_clip(
        &phrase,
        &HashMap::new(),
        None,
    )) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[phrasexp] expansion error: {e}");
            let _ = app.emit("phrase-error", format!("{e}"));
            erase_suppressed(inj, inj_flag, abbr.chars().count());
            buffer.clear();
            return;
        }
    };
    let global_on = case_adapt_flag.load(Ordering::Relaxed);
    let chunks = apply_case_mode_chunks(chunks, &abbr, phrase.case_mode, global_on);

    // Erase + inject on a WORKER THREAD. rdev listens via XRecord, which captures enigo's
    // XTest-injected keystrokes as echoes. If we injected synchronously here (on the listener
    // thread), those echoes would queue behind this callback and only be processed after we
    // return (with inj_flag already cleared) — re-entering the buffer and mangling output.
    // On a worker thread the listener stays free to process — and suppress — the echoes live.
    let n = abbr.chars().count();
    let content_type = phrase.content_type;
    inj_flag.store(true, Ordering::SeqCst);
    {
        // Clear the echo marker; the listener sets it to Some(instant) on the first echo.
        if let Ok(mut t) = last_echo.lock() {
            *t = None;
        }
    }
    buffer.clear();
    let state = app.state::<crate::AppState>();
    let db_arc = state.db.clone();
    let inj_arc = state.injector.clone();
    let phrase_id = phrase.id.clone();
    let flag = inj_flag.clone();
    let le = last_echo.clone();
    std::thread::spawn(move || {
        crate::triggers::inject::log_if_err(inj_arc.backspace(n), "erase abbreviation");
        crate::triggers::inject::log_if_err(
            crate::triggers::inject::deliver_chunks(&*inj_arc, content_type, &chunks),
            "deliver",
        );
        // Usage stats (1.4): counted off the listener thread — a WAL write has no
        // business sitting between a keystroke and its injection.
        let _ = db_arc.record_usage(&phrase_id);
        quiet_clear(&flag, &le);
    });
}

/// 1.4 delimiter mode: fire a parked pending match. The user typed `abbr` then a
/// delimiter (already delivered to the app — rdev can't suppress keys), so the
/// injection erases abbr + the delimiter, expands, and re-types the delimiter
/// after the expansion (or the raw Return/Tab key for key delimiters).
#[allow(clippy::too_many_arguments)]
pub(super) fn fire_delayed(
    pending: &PendingMatch,
    delim_char: Option<char>, // Some(c) for printable delimiters, None for Return/Tab
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    db: &Db,
    inj_flag: &Arc<AtomicBool>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
    case_adapt_flag: &AtomicBool,
    current: &Mutex<Option<String>>,
    app: &tauri::AppHandle,
) {
    let abbr = pending.abbr.clone();
    let ids = pending.ids.clone();
    // Erase abbreviation + delimiter, then either the chooser (multi) or the phrase.
    let erase = abbr.chars().count() + 1; // +1 for the delimiter
    inj_flag.store(true, Ordering::SeqCst);
    let state = app.state::<crate::AppState>();
    let inj_arc = state.injector.clone();
    if ids.len() > 1 {
        crate::triggers::inject::log_if_err(inj_arc.backspace(erase), "erase abbreviation");
        inj_flag.store(false, Ordering::SeqCst);
        buffer.clear();
        show_chooser(app, ids);
        return;
    }
    let id = ids[0].clone();
    // Cache-first, same as maybe_expand (index holds parsed phrases from rebuild).
    let phrase: std::sync::Arc<Phrase> = {
        let cached = idx.lock().ok().and_then(|i| i.cached_phrase(&id));
        match cached {
            Some(p) => p,
            None => match db.get_phrase(&id) {
                Ok(Some(p)) => std::sync::Arc::new(p),
                _ => {
                    // Phrase vanished while pending: leave the user's text as typed.
                    inj_flag.store(false, Ordering::SeqCst);
                    return;
                }
            },
        }
    };
    if phrase.deleted_at.is_some() || has_inputform(&phrase) {
        inj_flag.store(false, Ordering::SeqCst);
        return; // inputform phrases need the popup; the delimiter just stands.
    }
    if let Some(scope) = &phrase.app_scope {
        let cur = current.lock().map(|g| g.clone()).ok().flatten();
        if !crate::triggers::app_context::scope_active(scope, &cur) {
            inj_flag.store(false, Ordering::SeqCst);
            return;
        }
    }

    // AI phrases: erase + expand off-thread, then re-type the delimiter.
    if has_ai(&phrase) {
        let db_arc = state.db.clone();
        let phrase_owned = phrase.clone();
        let app_clone = app.clone();
        let case_adapt_on = case_adapt_flag.load(Ordering::Relaxed);
        let case_mode = phrase.case_mode;
        let abbr_owned = abbr.clone();
        let delim = delim_char;
        let flag = inj_flag.clone();
        let le = last_echo.clone();
        buffer.clear();
        std::thread::spawn(move || {
            crate::triggers::inject::log_if_err(inj_arc.backspace(erase), "erase abbreviation");
            match tauri::async_runtime::block_on(db_arc.expand_chunks_with_clip(
                &phrase_owned,
                &HashMap::new(),
                None,
            )) {
                Ok(chunks) => {
                    let chunks =
                        apply_case_mode_chunks(chunks, &abbr_owned, case_mode, case_adapt_on);
                    let _ = crate::triggers::inject::deliver_chunks(
                        &*inj_arc,
                        phrase_owned.content_type,
                        &chunks,
                    );
                    retype_delimiter(&inj_arc, delim);
                    let _ = db_arc.record_usage(&phrase_owned.id);
                }
                Err(e) => {
                    eprintln!("[phrasexp] AI expansion error: {e}");
                    let _ = app_clone.emit("phrase-error", format!("{e}"));
                    retype_delimiter(&inj_arc, delim);
                }
            }
            quiet_clear(&flag, &le);
        });
        return;
    }

    // Sync: expand, then worker thread erases + injects + re-types delimiter
    // (same echo-suppression shape as maybe_expand's sync path).
    let chunks = match tauri::async_runtime::block_on(db.expand_chunks_with_clip(
        &phrase,
        &HashMap::new(),
        None,
    )) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[phrasexp] expansion error: {e}");
            let _ = app.emit("phrase-error", format!("{e}"));
            buffer.clear();
            inj_flag.store(false, Ordering::SeqCst);
            return;
        }
    };
    let global_on = case_adapt_flag.load(Ordering::Relaxed);
    let chunks = apply_case_mode_chunks(chunks, &abbr, phrase.case_mode, global_on);
    buffer.clear();
    let content_type = phrase.content_type;
    let phrase_id = phrase.id.clone();
    let flag = inj_flag.clone();
    let le = last_echo.clone();
    let delim = delim_char;
    let db_arc = state.db.clone();
    let inj_sync = state.injector.clone();
    std::thread::spawn(move || {
        crate::triggers::inject::log_if_err(inj_sync.backspace(erase), "erase abbreviation");
        crate::triggers::inject::log_if_err(
            crate::triggers::inject::deliver_chunks(&*inj_sync, content_type, &chunks),
            "deliver",
        );
        retype_delimiter(&inj_sync, delim);
        // Usage stats off the listener thread (same rationale as maybe_expand).
        let _ = db_arc.record_usage(&phrase_id);
        quiet_clear(&flag, &le);
    });
}

/// Re-type the delimiter that fired a delayed expansion. Printable delimiters are
/// typed directly. Return/Tab (None) go through a fresh enigo key tap — the
/// Injector trait has no key-press method and typing a newline via type_text is
/// not equivalent in every app.
fn retype_delimiter(inj: &Arc<dyn Injector>, delim: Option<char>) {
    match delim {
        Some(c) => {
            crate::triggers::inject::log_if_err(inj.type_text(&c.to_string()), "retype delimiter");
        }
        None => {
            use enigo::{Key, Keyboard};
            if let Ok(mut e) = crate::triggers::inject::enigo() {
                let _ = e.key(Key::Return, enigo::Direction::Press);
                let _ = e.key(Key::Return, enigo::Direction::Release);
            }
        }
    }
}

/// Wait for injected echoes to quiesce, then clear the suppression flag
/// (shared tail of the delayed-mode injection workers). Sleeps on the shared
/// echo condvar instead of polling the clock: the listener pings after every
/// observed echo, so a worker wakes only when something actually happened.
/// Phase 1 waits (bounded 500 ms) for the first echo; phase 2 waits out the
/// 60 ms quiet gap after the last one. No echo at all → immediate release
/// after the phase-1 deadline, same as the old polling loop.
pub(super) fn quiet_clear(
    flag: &Arc<AtomicBool>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
) {
    let quiet_gap = std::time::Duration::from_millis(60);
    let mut guard = last_echo.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
    // Phase 1 — wait for the first echo, bounded (an expansion can produce none).
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while guard.is_none() {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        let (g, _t) = crate::triggers::inject::ECHO_CV
            .wait_timeout(guard, deadline - now)
            .unwrap_or_else(|e| e.into_inner());
        guard = g;
    }
    // Phase 2 — wait out the quiet gap after the last echo.
    loop {
        let elapsed_since_echo = guard.map(|t| t.elapsed()).unwrap_or_default();
        if guard.is_none() || elapsed_since_echo >= quiet_gap {
            break;
        }
        let (g, _t) = crate::triggers::inject::ECHO_CV
            .wait_timeout(guard, quiet_gap - elapsed_since_echo)
            .unwrap_or_else(|e| e.into_inner());
        guard = g;
    }
    flag.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::autotext::index::phrase;

    #[test]
    fn inputform_detected() {
        let with_input = phrase(
            "p",
            Some("hi"),
            vec![Segment::Macro {
                macro_value: Macro::InputForm {
                    field: "n".into(),
                    kind: Default::default(),
                    options: vec![],
                },
            }],
        );
        let plain = phrase("p", Some("hi"), vec![Segment::Text { text: "Hi".into() }]);
        assert!(has_inputform(&with_input));
        assert!(!has_inputform(&plain));
    }

    #[test]
    fn transition_pending_fires_on_delimiter_and_drops_otherwise() {
        let p = PendingMatch {
            abbr: "sig".into(),
            ids: vec!["p1".into()],
        };
        // fire: delimiter + buffer still ends with the abbr
        assert_eq!(
            transition_pending(&p, ' ', "hi sig"),
            PendingAction::Fire(' ')
        );
        for d in [',', '.', ';', ':', '!', '?'] {
            assert_eq!(transition_pending(&p, d, "hi sig"), PendingAction::Fire(d));
        }
        // drop: ordinary letter
        assert_eq!(transition_pending(&p, 'n', "hi sig"), PendingAction::Drop);
        // drop: delimiter but the buffer moved on (abbr no longer a suffix)
        assert_eq!(transition_pending(&p, ' ', "hi sign"), PendingAction::Drop);
    }

    #[test]
    fn is_delimiter_char_covers_punctuation_only() {
        assert!(is_delimiter_char(' '));
        assert!(is_delimiter_char(','));
        assert!(!is_delimiter_char('a'));
        assert!(!is_delimiter_char('/'));
    }

    #[test]
    fn apply_case_mode_respects_per_phrase_override() {
        use crate::domain::models::CaseMode;
        // body "hello there", abbr "SIG" → ALL CAPS adaptation
        let adapted = "HELLO THERE".to_string();
        let raw = "hello there".to_string();
        assert_eq!(
            apply_case_mode(raw.clone(), "SIG", CaseMode::Always, false),
            adapted
        );
        assert_eq!(
            apply_case_mode(raw.clone(), "sig", CaseMode::Always, false),
            raw.clone()
        );
        assert_eq!(
            apply_case_mode(raw.clone(), "SIG", CaseMode::Never, true),
            raw.clone()
        );
        assert_eq!(
            apply_case_mode(raw.clone(), "SIG", CaseMode::Adapt, true),
            adapted
        );
        assert_eq!(
            apply_case_mode(raw.clone(), "SIG", CaseMode::Adapt, false),
            raw
        );
    }
}
