mod commands;
mod db;
mod domain;
mod platform;
mod sync;
mod triggers;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, LogicalSize, Manager, Size, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::domain::models::{csv_set, TriggerType};
use crate::triggers::autotext::AppChord;

/// Shared application state.
pub struct AppState {
    pub db: Arc<db::Db>,
    pub injector: Arc<dyn triggers::Injector>,
    pub abbrev_index: Arc<Mutex<triggers::AbbrevIndex>>,
    pub autotext_enabled: Arc<AtomicBool>,
    pub spellcheck_enabled: Arc<AtomicBool>,
    pub case_adapt_enabled: Arc<AtomicBool>,
    /// Opt-in "review sentence": AI grammar pass offered after sentence end (live copy).
    pub review_sentence: Arc<AtomicBool>,
    /// Keystroke counter — lets the async sentence review detect typing since trigger.
    pub typed_counter: Arc<AtomicU64>,
    pub spellcheck_ignore: Arc<Mutex<std::collections::HashSet<String>>>,
    pub spellcheck_mode: Arc<Mutex<String>>,
    /// Learned typo→correction pairs (learn-on-accept, unlearn-on-undo). Checked before
    /// the SymSpell traversal, so habitual typos correct instantly and deterministically.
    pub spellcheck_learned: Arc<Mutex<HashMap<String, String>>>,
    pub spellcheck_hotkey: Arc<Mutex<String>>,
    pub spellcheck_exclude_apps: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Window-title substrings where autotext expansion is fully disabled (live copy
    /// of the settings CSV, same pattern as spellcheck_exclude_apps).
    pub autotext_exclude_apps: Arc<Mutex<std::collections::HashSet<String>>>,
    /// "silent" (auto-correct inline) or "offer" (pop up candidates; correct only on Accept).
    pub spellcheck_delivery: Arc<Mutex<String>>,
    /// Echo-suppression flag shared between the listener and the apply_correction command,
    /// so injected keystrokes aren't re-processed by the listener.
    pub autotext_injecting: Arc<AtomicBool>,
    pub autotext_last_echo: Arc<Mutex<Option<std::time::Instant>>>,
    /// Offer-mode: the original form of the currently-offered correction (None = none pending).
    /// Cleared by any real keystroke (staleness guard).
    pub spellcheck_offer: Arc<Mutex<Option<String>>>,
    /// Candidates emitted with the current offer; apply_correction validates against this set.
    pub spellcheck_offer_candidates: Arc<Mutex<Vec<String>>>,
    /// Handshake flag: the listener clears its buffer when set (an accepted correction
    /// rewrites the screen out from under the thread-local buffer).
    pub spellcheck_buffer_clear: Arc<AtomicBool>,
    pub autotext_running: Arc<AtomicBool>,
    pub autotext_events: Arc<AtomicU64>,
    pub clipboard_enabled: Arc<AtomicBool>,
    pub clipboard_max_length: Arc<std::sync::atomic::AtomicU32>,
    pub clipboard_max_entries: Arc<std::sync::atomic::AtomicU32>,
    /// Registered per-phrase hotkeys: (shortcut, phrase_id).
    pub hotkey_registry: Arc<Mutex<Vec<(Shortcut, String)>>>,
    /// App hotkeys delivered by the key listener on platforms where the
    /// global-shortcut plugin can't grab (Wayland) or was refused. Rebuilt by
    /// `reregister_hotkeys`; checked on every key event.
    pub app_chords: Arc<Mutex<Vec<triggers::autotext::AppChord>>>,
    /// Title of the currently focused window (for app-scoped phrases).
    pub current_app: Arc<Mutex<Option<String>>>,
    /// Pending selection-AI context: the captured selection plus the user's prior
    /// clipboard, held between hotkey capture and the run_ai_action command (or an
    /// Esc-dismiss, which leaves the slot stale until the next capture overwrites it).
    pub ai_selection: Arc<Mutex<Option<AiSelection>>>,
    /// AI response shown in the result popup, pending the user's decision
    /// (replace / insert / retry / discard). Peeks on retry, taken on terminal choices.
    pub ai_result: Arc<Mutex<Option<AiPending>>>,
    /// Set by the ai_cancel command to abort an in-flight streaming AI request at
    /// the next chunk; cleared at the start of every run/retry.
    pub ai_cancel: Arc<AtomicBool>,
    /// True while the app-context poller needs the window title (app-scoped phrases
    /// exist, or spellcheck excludes apps); false lets the poller skip forking xdotool.
    pub app_poll_needed: Arc<AtomicBool>,
}

impl AppState {
    /// Recompute `app_poll_needed` from current settings + phrase library. Errs toward
    /// polling (any scoped phrase, even in a disabled folder, keeps the poller live).
    pub fn refresh_app_poll_needed(&self) {
        let excl = self
            .spellcheck_exclude_apps
            .lock()
            .map(|g| !g.is_empty())
            .unwrap_or(false);
        let scoped = self
            .db
            .list_phrases(None, None, false)
            .map(|ps| {
                ps.iter()
                    .any(|p| p.app_scope.as_deref().is_some_and(|s| !s.is_empty()))
            })
            .unwrap_or(false);
        self.app_poll_needed.store(
            (self.spellcheck_enabled.load(Ordering::Relaxed) && excl) || scoped,
            Ordering::Relaxed,
        );
    }
}

/// Runtime diagnostics: run the app with `PHRASEXP_DEBUG=1` to trace key events, buffer
/// matches, injections, and popup shows on stderr. Off by default — every feature here
/// fails silently by nature (no window to print into), so this is the lever for the
/// "nothing happens at all" class of bug.
pub(crate) fn debug_log(msg: &str) {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var("PHRASEXP_DEBUG").as_deref() == Ok("1")) {
        eprintln!("[phrasexp] {msg}");
    }
}

/// Background poller keeping `current_app` fresh (app-scoping + spellcheck exclude).
/// Only the focused-window title is queried (one cheap X round-trip); app scoping/exclude
/// tolerate a few seconds of staleness, so the interval is leisurely to minimize idle wakeups.
/// The xdotool fork+exec only happens while `needed` is set (see `AppState::refresh_app_poll_needed`);
/// otherwise a tick is a bool compare + sleep.
fn start_app_poller(current: Arc<Mutex<Option<String>>>, needed: Arc<AtomicBool>) {
    std::thread::spawn(move || loop {
        if needed.load(Ordering::Relaxed) {
            crate::debug_log("app poll: querying window title");
            let title = triggers::app_context::current_window_title();
            if let Ok(mut g) = current.lock() {
                *g = title;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(2500));
    });
}

/// Data directory (portable build: next to the exe; else the OS app-data dir).
/// The DB and pasted images both live here.
pub fn data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        if exe_dir.join("phrasexp.portable").exists() {
            return exe_dir.to_path_buf();
        }
    }
    let dir = app.path().app_data_dir().expect("app data dir");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Reserved hotkey-registry target for the selection-AI hotkey (not a phrase id).
const SELECTION_AI_TOKEN: &str = "__selection_ai__";
/// Reserved listener token for the search-popup hotkey (settings.popup_hotkey).
const APP_POPUP_TOKEN: &str = "__popup__";
/// Reserved hotkey-registry target for repeat-last-output.
const REPEAT_OUTPUT_TOKEN: &str = "__repeat_output__";
/// Fixed chord for repeat-last-output (shown in Settings → Shortcuts).
const REPEAT_OUTPUT_HOTKEY: &str = "Control+Alt+R";

/// Selection captured by the selection-AI hotkey, plus the user's prior clipboard so
/// `run_ai_action` can restore it after pasting the rewrite in place.
#[derive(Debug, Clone)]
pub struct AiSelection {
    pub selection: String,
    pub old_clipboard: Option<String>,
}

/// An AI rewrite awaiting the user's decision in the result popup. Holds everything
/// `ai_decide` needs — Replace/Insert paste `result`, Retry re-runs `action_id` over
/// `selection` (with the answers keyed by their `{token}` names for
/// `{language}`/`{length}`-style templates), and every terminal path restores
/// `old_clipboard`.
#[derive(Debug, Clone)]
pub struct AiPending {
    pub action_id: String,
    pub result: String,
    pub selection: String,
    pub params: Option<std::collections::HashMap<String, String>>,
    pub old_clipboard: Option<String>,
}

/// Selection-based AI (Grammarly's "rewrite selection" as a local flow): copy the
/// focused app's selection via the clipboard, park it + the prior clipboard in
/// AppState, then open the popup in AI mode. Spawns a thread — the capture sleeps.
fn start_selection_ai(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let settings = match state.db.load_settings() {
        Ok(s) => s,
        Err(e) => {
            let _ = app.emit("phrase-error", format!("{e}"));
            return;
        }
    };
    if !settings.ai_enabled || settings.ai_api_key.is_empty() {
        let _ = app.emit(
            "phrase-error",
            "Selection AI needs AI enabled + an API key in Settings".to_string(),
        );
        return;
    }
    let injector = Arc::clone(&state.injector);
    let slot = Arc::clone(&state.ai_selection);
    let app_h = app.clone();
    std::thread::spawn(move || {
        let mut clip = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                let _ = app_h.emit("phrase-error", format!("clipboard: {e}"));
                return;
            }
        };
        let old = clip.get_text().ok().filter(|s| !s.is_empty());
        // Clear so a failed copy can't be mistaken for the selection.
        let _ = clip.clear();
        std::thread::sleep(std::time::Duration::from_millis(80));
        // The user's fingers may still hold the hotkey (Ctrl+Alt): a copy sent now
        // lands as Ctrl+Alt+C, which most apps reject. Wait — bounded — until the
        // listener sees every chord modifier released (see CHORD_MODS_HELD).
        {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            while crate::triggers::autotext::CHORD_MODS_HELD.load(Ordering::SeqCst)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        if let Err(e) = injector.copy_selection() {
            let _ = app_h.emit("phrase-error", format!("{e}"));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let selection = clip.get_text().ok().filter(|s| !s.is_empty());
        let Some(selection) = selection else {
            if let Some(prev) = old {
                let _ = clip.set_text(&prev);
            }
            // Surface in the popup itself — the main window is usually hidden to
            // tray, so phrase-error toasts are never seen.
            show_popup(&app_h);
            let _ = app_h.emit(
                "ai-selection-error",
                "No selection captured — select text first".to_string(),
            );
            return;
        };
        // Truncated copy for the popup's "what's captured" line — the full selection
        // stays in the AppState slot; the popup never round-trips it.
        let preview: String = selection.chars().take(120).collect();
        if let Ok(mut g) = slot.lock() {
            *g = Some(AiSelection {
                selection,
                old_clipboard: old,
            });
        }
        show_popup(&app_h);
        let _ = app_h.emit("ai-selection", preview);
    });
}

/// Repeat-last-output (PhraseExpress v18): re-deliver the last successful expansion
/// into the focused app — for when it landed in the wrong window or is needed twice.
/// Keeps the recording (peek, not take) so the chord can be pressed repeatedly.
fn repeat_last_output(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if let Some((content_type, chunks)) = crate::triggers::inject::last_delivery() {
        let _ = crate::triggers::inject::deliver_chunks(&*state.injector, content_type, &chunks);
    }
}

/// Route a listener-detected hotkey token (see `reregister_hotkeys`). Runs on
/// the main thread — the same context the global-shortcut handler used.
fn dispatch_app_token(app: &tauri::AppHandle, token: &str) {
    match token {
        APP_POPUP_TOKEN => show_popup(app),
        SELECTION_AI_TOKEN => start_selection_ai(app),
        REPEAT_OUTPUT_TOKEN => repeat_last_output(app),
        id => inject_phrase_by_id(app, id),
    }
}

fn show_popup(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window("popup") else {
        return;
    };
    // A visible popup stays where the user (or the flow) put it — re-summoning mid
    // session (e.g. the AI result arriving) must not yank it back to the cursor.
    // Only a cold summon appears near the cursor; the frontend then sizes the
    // window to its content (resize_popup), so the full default is a cold-start
    // frame only.
    if !win.is_visible().unwrap_or(false) {
        position_popup_at_cursor(&win);
        // Restore full size: a prior compact spellcheck offer may have shrunk this
        // window (content-driven resize corrects it after render).
        let _ = win.set_size(Size::Logical(LogicalSize::new(560.0, 420.0)));
    }
    summon_popup(app, &win);
    let _ = app.emit("popup-shown", ());
}

/// Move the popup window next to the mouse cursor (8px offset, clamped off the
/// screen origin). Shared by every cold popup summon (search, chooser, offer).
pub(crate) fn position_popup_at_cursor(win: &tauri::WebviewWindow) {
    use device_query::{DeviceQuery, DeviceState};
    let (mx, my) = DeviceState::new().get_mouse().coords;
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        (mx + 8).max(0),
        (my + 8).max(0),
    )));
}

/// Bring the popup on screen with key focus. On macOS the app must be activated
/// first (background apps can't make key windows, and no keyboard grab exists to
/// compensate).
pub(crate) fn summon_popup(
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] app: &tauri::AppHandle,
    win: &tauri::WebviewWindow,
) {
    #[cfg(target_os = "macos")]
    let _ = app.show();
    let _ = win.show();
    let _ = win.set_focus();
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Inject a phrase by id directly into the focused app (per-phrase hotkeys).
fn inject_phrase_by_id(app: &tauri::AppHandle, id: &str) {
    let state = app.state::<AppState>();
    if let Ok(phrase) = state.db.get_active_phrase(id) {
        // Global expansion exclusion applies to explicit hotkeys too — a disabled
        // window (password manager, terminal) stays disabled for all autotext output.
        let excluded = state
            .current_app
            .lock()
            .ok()
            .and_then(|cur| {
                let exset = state.autotext_exclude_apps.lock().ok()?;
                crate::triggers::app_context::title_matches_any(&cur, &exset).then_some(true)
            })
            .unwrap_or(false);
        if excluded {
            return;
        }
        // Respect app scope.
        let cur = state.current_app.lock().map(|g| g.clone()).ok().flatten();
        let active = phrase
            .app_scope
            .as_deref()
            .is_none_or(|s| triggers::app_context::scope_active(s, &cur));
        if !active {
            return;
        }
        if let Ok(chunks) = tauri::async_runtime::block_on(state.db.expand_chunks_with_clip(
            &phrase,
            &std::collections::HashMap::new(),
            None,
        )) {
            let _ = crate::triggers::inject::deliver_chunks(
                &*state.injector,
                phrase.content_type,
                &chunks,
            );
        }
    }
}

/// Re-register the popup hotkey + all per-phrase hotkeys; rebuild the lookup registry.
///
/// Two delivery paths, chosen per platform:
/// - **Linux:** the global-shortcut plugin is X11-only (`XGrabKey`), and on Wayland
///   those grabs never fire while a native-Wayland window has focus — so every chord
///   below is routed through the key listener (`AppChord` table) instead.
/// - **Windows/macOS:** global shortcuts work and consume the keystroke; the listener
///   table only takes chords the OS refused (conflict), as a fallback.
pub fn reregister_hotkeys(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut reg: Vec<(Shortcut, String)> = Vec::new();
    let mut chords: Vec<AppChord> = Vec::new();
    let gs_works = !cfg!(target_os = "linux");

    // Chord sink: register via gs where it works, else hand to the listener.
    macro_rules! add_chord {
        ($hotkey:expr, $token:expr) => {{
            let hotkey: &str = $hotkey;
            let token: &str = $token;
            let mut delivered = false;
            if gs_works {
                if let Some(sc) = parse_hotkey(hotkey) {
                    delivered = gs.register(sc).is_ok();
                    if delivered {
                        reg.push((sc, token.to_string()));
                    }
                }
            }
            if !delivered {
                if let Some(c) = triggers::autotext::AppChord::parse(hotkey, token) {
                    chords.push(c);
                }
            }
        }};
    }

    // popup hotkey (from settings). Spellcheck toggle is handled by the key listener.
    let settings = state.db.load_settings().ok();
    if let Some(settings) = &settings {
        add_chord!(&settings.popup_hotkey, APP_POPUP_TOKEN);
        // selection-AI hotkey — reserved token routes to start_selection_ai
        // instead of the search popup.
        if !settings.ai_selection_hotkey.trim().is_empty() {
            add_chord!(&settings.ai_selection_hotkey, SELECTION_AI_TOKEN);
        }
    }
    // repeat-last-output — fixed chord, always on (skipped if the user assigned the
    // same chord to the selection-AI hotkey; first registration would win anyway).
    let selection_hotkey = settings
        .as_ref()
        .map(|s| s.ai_selection_hotkey.as_str())
        .unwrap_or_default();
    if REPEAT_OUTPUT_HOTKEY != selection_hotkey.trim() {
        add_chord!(REPEAT_OUTPUT_HOTKEY, REPEAT_OUTPUT_TOKEN);
    }
    // per-phrase hotkeys (skip phrases in disabled folders)
    let enabled_folders: std::collections::HashSet<String> = state
        .db
        .list_folders()
        .map(|fs| {
            fs.iter()
                .filter(|f| f.enabled)
                .map(|f| f.id.clone())
                .collect()
        })
        .unwrap_or_default();
    if let Ok(phrases) = state.db.list_phrases(None, None, false) {
        for p in phrases {
            if !enabled_folders.contains(&p.folder_id) {
                continue;
            }
            for t in &p.triggers {
                if t.kind == TriggerType::Hotkey {
                    if let Some(key) = &t.key {
                        add_chord!(key.as_str(), p.id.as_str());
                    }
                }
            }
        }
    }
    let registry = state.hotkey_registry.clone();
    let _ = registry.lock().map(|mut g| *g = reg);
    let _ = state.app_chords.lock().map(|mut g| *g = chords);
}

/// Parse a hotkey string like "Control+Alt+Space" into a Shortcut.
pub fn parse_hotkey(s: &str) -> Option<Shortcut> {
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for raw in s.split('+') {
        let part = raw.trim().to_lowercase();
        if part.is_empty() {
            continue;
        }
        match part.as_str() {
            "control" | "ctrl" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" | "opt" => mods |= Modifiers::ALT,
            "meta" | "super" | "win" | "cmd" | "command" => mods |= Modifiers::META,
            other => code = Some(name_to_code(other)?),
        }
    }
    Some(Shortcut::new(
        if mods.is_empty() { None } else { Some(mods) },
        code?,
    ))
}

fn name_to_code(name: &str) -> Option<Code> {
    use Code::*;
    Some(match name {
        "space" => Space,
        "\\" => Backslash,
        "," | "comma" => Comma,
        "return" | "enter" => Enter,
        "tab" => Tab,
        "backspace" => Backspace,
        "escape" | "esc" => Escape,
        "a" => KeyA,
        "b" => KeyB,
        "c" => KeyC,
        "d" => KeyD,
        "e" => KeyE,
        "f" => KeyF,
        "g" => KeyG,
        "h" => KeyH,
        "i" => KeyI,
        "j" => KeyJ,
        "k" => KeyK,
        "l" => KeyL,
        "m" => KeyM,
        "n" => KeyN,
        "o" => KeyO,
        "p" => KeyP,
        "q" => KeyQ,
        "r" => KeyR,
        "s" => KeyS,
        "t" => KeyT,
        "u" => KeyU,
        "v" => KeyV,
        "w" => KeyW,
        "x" => KeyX,
        "y" => KeyY,
        "z" => KeyZ,
        "0" => Digit0,
        "1" => Digit1,
        "2" => Digit2,
        "3" => Digit3,
        "4" => Digit4,
        "5" => Digit5,
        "6" => Digit6,
        "7" => Digit7,
        "8" => Digit8,
        "9" => Digit9,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        _ => return None,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch hands off here instead of
        // starting a second key listener + injector.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        // Per-phrase hotkey? -> inject that phrase directly. Else -> popup.
                        let phrase_id = app
                            .state::<AppState>()
                            .hotkey_registry
                            .lock()
                            .map(|reg| {
                                reg.iter()
                                    .find(|(sc, _)| sc == shortcut)
                                    .map(|(_, id)| id.clone())
                            })
                            .ok()
                            .flatten();
                        if let Some(id) = phrase_id {
                            if id == SELECTION_AI_TOKEN {
                                start_selection_ai(app);
                            } else if id == REPEAT_OUTPUT_TOKEN {
                                repeat_last_output(app);
                            } else {
                                inject_phrase_by_id(app, &id);
                            }
                        } else {
                            show_popup(app);
                        }
                    }
                })
                .build(),
        )
        .on_window_event(|window, event| {
            // Run in background: closing the main window hides it (tray still active).
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            // Diagnostic: what each webview actually loaded. A popup stuck on an error
            // page ("Could not connect to localhost") shows up here immediately.
            use tauri::Manager;
            for label in ["main", "popup"] {
                if let Some(w) = app.get_webview_window(label) {
                    crate::debug_log(&format!("webview {label} url: {:?}", w.url()));
                }
            }
            // Portable mode: DB lives next to the executable if a
            // `phrasexp.portable` marker is present; otherwise the OS app-data dir.
            let db_path = data_dir(app.handle()).join("phrasexp.db");
            let db = Arc::new(db::Db::open(db_path.to_str().expect("valid db path"))?);

            // Attach the encryption layer before any read so pre-existing enveloped
            // values decrypt. The DEK lives in the OS keychain; if the keychain is unavailable
            // (e.g. headless Linux with no Secret Service) encryption is inert (plaintext) —
            // the app still works, enveloped rows just degrade to locked placeholders.
            let crypto = match db::crypto::KeyringStore::new() {
                Ok(store) => db::Crypto::init(&store),
                Err(e) => {
                    eprintln!("[phrasexp] keychain unavailable — encryption disabled: {e}");
                    db::Crypto::disabled()
                }
            };
            db.set_crypto(std::sync::Arc::new(crypto));

            // Load settings + apply runtime flags.
            let settings = db.load_settings()?;

            // One-time, idempotent: envelope sensitive data that predates the feature.
            if settings.encrypt_at_rest && db.crypto_available() {
                if let Err(e) = db.migrate_encrypt_at_rest() {
                    eprintln!("[phrasexp] at-rest encryption migration failed: {e}");
                }
            }
            // Start minimized to tray: hide the main window immediately. The tray icon is
            // built below and its "Show" entry / click still reopens it.
            if settings.start_minimized {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
            }
            let autotext_enabled = Arc::new(AtomicBool::new(settings.autotext_enabled));
            let clipboard_enabled = Arc::new(AtomicBool::new(settings.clipboard_enabled));
            let clipboard_max_length = Arc::new(std::sync::atomic::AtomicU32::new(
                settings.clipboard_max_length,
            ));
            let clipboard_max_entries = Arc::new(std::sync::atomic::AtomicU32::new(
                settings.clipboard_max_entries,
            ));
            let spellcheck_enabled = Arc::new(AtomicBool::new(settings.spellcheck_enabled));
            let case_adapt_enabled = Arc::new(AtomicBool::new(settings.case_adapt_enabled));
            let review_sentence = Arc::new(AtomicBool::new(settings.review_sentence));
            let typed_counter = Arc::new(AtomicU64::new(0));
            let spellcheck_ignore = Arc::new(Mutex::new(csv_set(&settings.spellcheck_ignore)));
            let spellcheck_mode = Arc::new(Mutex::new(settings.spellcheck_mode.clone()));
            let spellcheck_learned: Arc<Mutex<HashMap<String, String>>> =
                Arc::new(Mutex::new(db.list_spellcheck_learned().unwrap_or_default()));
            let spellcheck_hotkey = Arc::new(Mutex::new(settings.spellcheck_hotkey.clone()));
            let spellcheck_exclude_apps =
                Arc::new(Mutex::new(csv_set(&settings.spellcheck_exclude_apps)));
            let autotext_exclude_apps =
                Arc::new(Mutex::new(csv_set(&settings.autotext_exclude_apps)));
            let spellcheck_delivery = Arc::new(Mutex::new(settings.spellcheck_delivery.clone()));
            // Echo-suppression state lives in AppState so both the listener thread and the
            // apply_correction command (main thread) share one flag/timer.
            let autotext_injecting = Arc::new(AtomicBool::new(false));
            let autotext_last_echo: Arc<Mutex<Option<std::time::Instant>>> =
                Arc::new(Mutex::new(None));
            let spellcheck_offer = Arc::new(Mutex::new(None::<String>));
            let spellcheck_offer_candidates = Arc::new(Mutex::new(Vec::<String>::new()));
            let spellcheck_buffer_clear = Arc::new(AtomicBool::new(false));

            let injector = triggers::make_injector();

            // Abbreviation index + autotext listener.
            let phrases = db.list_phrases(None, None, false)?;
            let abbrev_index = Arc::new(Mutex::new(triggers::AbbrevIndex::from_phrases(&phrases)));
            {
                let guard = abbrev_index.lock();
                crate::debug_log(&format!(
                    "abbrev index: {} literal(s) from {} phrase(s)",
                    guard.as_ref().map(|i| i.len()).unwrap_or(0),
                    phrases.len()
                ));
            }
            let current_app = Arc::new(Mutex::new(None::<String>));
            let excl_nonempty = !settings.spellcheck_exclude_apps.is_empty();
            let scoped = phrases
                .iter()
                .any(|p| p.app_scope.as_deref().is_some_and(|s| !s.is_empty()));
            let sc_on = settings.spellcheck_enabled;
            crate::debug_log(&format!(
                "app poller: spellcheck={sc_on} excl_nonempty={excl_nonempty} scoped={scoped}"
            ));
            let app_poll_needed = Arc::new(AtomicBool::new(
                (settings.spellcheck_enabled && excl_nonempty) || scoped,
            ));
            start_app_poller(Arc::clone(&current_app), Arc::clone(&app_poll_needed));
            // App-level hotkey chords for the key listener; populated by
            // reregister_hotkeys once AppState is managed.
            let app_chords: Arc<Mutex<Vec<triggers::autotext::AppChord>>> =
                Arc::new(Mutex::new(Vec::new()));
            let autotext = triggers::autotext::start(
                Arc::clone(&db),
                Arc::clone(&injector),
                Arc::clone(&abbrev_index),
                Arc::clone(&autotext_enabled),
                Arc::clone(&spellcheck_enabled),
                Arc::clone(&case_adapt_enabled),
                Arc::clone(&review_sentence),
                Arc::clone(&typed_counter),
                Arc::clone(&current_app),
                Arc::clone(&spellcheck_ignore),
                Arc::clone(&spellcheck_mode),
                Arc::clone(&spellcheck_learned),
                Arc::clone(&spellcheck_hotkey),
                Arc::clone(&spellcheck_exclude_apps),
                Arc::clone(&autotext_exclude_apps),
                Arc::clone(&spellcheck_delivery),
                Arc::clone(&autotext_injecting),
                Arc::clone(&autotext_last_echo),
                Arc::clone(&spellcheck_offer),
                Arc::clone(&spellcheck_offer_candidates),
                Arc::clone(&spellcheck_buffer_clear),
                Arc::clone(&app_chords),
                app.handle().clone(),
            );

            // Clipboard auto-capture poller.
            triggers::clipboard::start(
                Arc::clone(&db),
                Arc::clone(&clipboard_enabled),
                Arc::clone(&clipboard_max_length),
                Arc::clone(&clipboard_max_entries),
            );

            app.manage(AppState {
                db,
                injector,
                abbrev_index,
                autotext_enabled,
                spellcheck_enabled,
                case_adapt_enabled,
                review_sentence,
                typed_counter,
                spellcheck_ignore,
                spellcheck_mode,
                spellcheck_learned,
                spellcheck_hotkey,
                spellcheck_exclude_apps,
                autotext_exclude_apps: Arc::clone(&autotext_exclude_apps),
                spellcheck_delivery,
                autotext_injecting,
                autotext_last_echo,
                spellcheck_offer,
                spellcheck_offer_candidates,
                spellcheck_buffer_clear,
                autotext_running: autotext.running,
                autotext_events: autotext.events_seen,
                clipboard_enabled,
                clipboard_max_length,
                clipboard_max_entries,
                hotkey_registry: Arc::new(Mutex::new(Vec::new())),
                app_chords: Arc::clone(&app_chords),
                current_app,
                app_poll_needed,
                ai_selection: Arc::new(Mutex::new(None)),
                ai_result: Arc::new(Mutex::new(None)),
                ai_cancel: Arc::new(AtomicBool::new(false)),
            });

            // Register the popup hotkey + all per-phrase hotkeys.
            #[cfg(desktop)]
            {
                reregister_hotkeys(app.handle());
            }

            // System tray (Show / Quit).
            let show = MenuItem::with_id(app, "show", "Show PhraseXp", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("PhraseXp")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            // Sync launch-on-login with stored settings.
            let autolaunch = app.autolaunch();
            if let Ok(enabled) = autolaunch.is_enabled() {
                if enabled != settings.autostart {
                    let _ = if settings.autostart {
                        autolaunch.enable()
                    } else {
                        autolaunch.disable()
                    };
                }
            }

            // File-based sync: pull + push once at startup when a sync folder is
            // configured (best-effort — failures surface via sync_status in
            // Settings → Data, never a modal).
            {
                let db = app.state::<AppState>().db.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if let Err(e) = crate::sync::sync_now(&db) {
                        crate::debug_log(&format!("startup sync: {e}"));
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_folders,
            commands::create_folder,
            commands::delete_folder,
            commands::merge_folders,
            commands::set_folder_enabled,
            commands::list_phrases,
            commands::get_phrase,
            commands::create_phrase,
            commands::update_phrase,
            commands::delete_phrase,
            commands::save_image,
            commands::read_image,
            commands::expand_phrase,
            commands::resolve_phrase,
            commands::inject_phrase,
            commands::list_clipboard,
            commands::add_clipboard,
            commands::clear_clipboard,
            commands::reset_database,
            commands::folder_counts,
            commands::test_ai,
            commands::list_ai_models,
            commands::list_ai_actions,
            commands::save_ai_action,
            commands::delete_ai_action,
            commands::run_ai_action,
            commands::ai_decide,
            commands::ai_cancel,
            commands::ai_variations,
            commands::fetch_citation_metadata,
            commands::format_citation,
            commands::sync_now,
            commands::sync_set_dir,
            commands::sync_status,
            commands::export_library,
            commands::import_csv,
            commands::import_ahk,
            commands::import_atext,
            commands::import_textexpander,
            commands::import_espanso,
            commands::import_json,
            commands::find_duplicates,
            commands::list_versions,
            commands::restore_version,
            commands::autotext_status,
            commands::get_settings,
            commands::set_settings,
            commands::crypto_status,
            commands::apply_correction,
            commands::delete_offer_word,
            commands::spellcheck_ignore_word,
            commands::focus_popup,
            commands::resize_popup,
            commands::grab_keyboard,
            commands::release_keyboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
