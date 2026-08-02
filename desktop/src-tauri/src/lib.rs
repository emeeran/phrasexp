mod commands;
mod db;
mod domain;
mod triggers;

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::domain::models::TriggerType;

/// Shared application state.
pub struct AppState {
    pub db: Arc<db::Db>,
    pub injector: Arc<dyn triggers::Injector>,
    pub abbrev_index: Arc<Mutex<triggers::AbbrevIndex>>,
    pub autotext_enabled: Arc<AtomicBool>,
    pub autotext_running: Arc<AtomicBool>,
    pub autotext_events: Arc<AtomicU64>,
    pub clipboard_enabled: Arc<AtomicBool>,
    /// Registered per-phrase hotkeys: (shortcut, phrase_id).
    pub hotkey_registry: Arc<Mutex<Vec<(Shortcut, String)>>>,
    /// Phase 2: title of the currently focused window (for app-scoped phrases).
    pub current_app: Arc<Mutex<Option<String>>>,
}

/// Background poller keeping `current_app` fresh (Phase 2 app-scoping).
fn start_app_poller(current: Arc<Mutex<Option<String>>>) {
    std::thread::spawn(move || loop {
        let title = triggers::app_context::current_window_title();
        if let Ok(mut g) = current.lock() {
            *g = title;
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
    });
}

/// Phase 8: resolve the DB path — portable (next to the exe, if `phrasexp.portable`
/// marker exists) or the OS app-data directory otherwise.
fn resolve_db_path(app: &tauri::App) -> std::path::PathBuf {
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        if exe_dir.join("phrasexp.portable").exists() {
            return exe_dir.join("phrasexp.db");
        }
    }
    let dir = app.path().app_data_dir().expect("app data dir");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("phrasexp.db")
}

/// Summon the search popup near the cursor (M3 UX).
fn show_popup(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window("popup") else {
        return;
    };
    let (mx, my) = {
        use device_query::{DeviceQuery, DeviceState};
        DeviceState::new().get_mouse().coords
    };
    let x = (mx + 8).max(0);
    let y = (my + 8).max(0);
    let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit("popup-shown", ());
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
    if let Ok(Some(phrase)) = state.db.get_phrase(id) {
        if phrase.deleted_at.is_some() {
            return;
        }
        // Phase 2: respect app scope.
        let cur = state
            .current_app
            .lock()
            .map(|g| g.clone())
            .ok()
            .flatten();
        let active = phrase
            .app_scope
            .as_deref()
            .is_none_or(|s| triggers::app_context::scope_active(s, &cur));
        if !active {
            return;
        }
        if let Ok(output) = state.db.expand(&phrase, &std::collections::HashMap::new()) {
            let _ = state.injector.type_text(&output);
        }
    }
}

/// Re-register the popup hotkey + all per-phrase hotkeys; rebuild the lookup registry.
pub fn reregister_hotkeys(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut reg: Vec<(Shortcut, String)> = Vec::new();

    // popup hotkey (from settings)
    if let Ok(settings) = state.db.load_settings() {
        if let Some(sc) = parse_hotkey(&settings.popup_hotkey) {
            let _ = gs.register(sc);
        }
    }
    // per-phrase hotkeys
    if let Ok(phrases) = state.db.list_phrases(None, None, false) {
        for p in phrases {
            for t in &p.triggers {
                if t.kind == TriggerType::Hotkey {
                    if let Some(key) = &t.key {
                        if let Some(sc) = parse_hotkey(key) {
                            let _ = gs.register(sc);
                            reg.push((sc, p.id.clone()));
                        }
                    }
                }
            }
        }
    }
    let registry = state.hotkey_registry.clone();
    let _ = registry.lock().map(|mut g| *g = reg);
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
        "return" | "enter" => Enter,
        "tab" => Tab,
        "backspace" => Backspace,
        "escape" | "esc" => Escape,
        "a" => KeyA, "b" => KeyB, "c" => KeyC, "d" => KeyD, "e" => KeyE, "f" => KeyF,
        "g" => KeyG, "h" => KeyH, "i" => KeyI, "j" => KeyJ, "k" => KeyK, "l" => KeyL,
        "m" => KeyM, "n" => KeyN, "o" => KeyO, "p" => KeyP, "q" => KeyQ, "r" => KeyR,
        "s" => KeyS, "t" => KeyT, "u" => KeyU, "v" => KeyV, "w" => KeyW, "x" => KeyX,
        "y" => KeyY, "z" => KeyZ,
        "0" => Digit0, "1" => Digit1, "2" => Digit2, "3" => Digit3, "4" => Digit4,
        "5" => Digit5, "6" => Digit6, "7" => Digit7, "8" => Digit8, "9" => Digit9,
        "f1" => F1, "f2" => F2, "f3" => F3, "f4" => F4, "f5" => F5, "f6" => F6,
        "f7" => F7, "f8" => F8, "f9" => F9, "f10" => F10, "f11" => F11, "f12" => F12,
        _ => return None,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
                            inject_phrase_by_id(app, &id);
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
            // FR-11 + Phase 8 portable mode: DB lives next to the executable if a
            // `phrasexp.portable` marker is present; otherwise the OS app-data dir.
            let db_path = resolve_db_path(app);
            let db = Arc::new(db::Db::open(db_path.to_str().expect("valid db path"))?);

            // M5: load settings + apply runtime flags.
            let settings = db.load_settings()?;
            let autotext_enabled = Arc::new(AtomicBool::new(settings.autotext_enabled));
            let clipboard_enabled = Arc::new(AtomicBool::new(settings.clipboard_enabled));

            let injector = triggers::make_injector();

            // M4: abbreviation index + autotext listener.
            let phrases = db.list_phrases(None, None, false)?;
            let abbrev_index =
                Arc::new(Mutex::new(triggers::AbbrevIndex::from_phrases(&phrases)));
            let current_app = Arc::new(Mutex::new(None::<String>));
            start_app_poller(Arc::clone(&current_app));
            let autotext = triggers::autotext::start(
                Arc::clone(&db),
                Arc::clone(&injector),
                Arc::clone(&abbrev_index),
                Arc::clone(&autotext_enabled),
                Arc::clone(&current_app),
                app.handle().clone(),
            );

            // M5: clipboard auto-capture poller.
            triggers::clipboard::start(Arc::clone(&db), Arc::clone(&clipboard_enabled));

            app.manage(AppState {
                db,
                injector,
                abbrev_index,
                autotext_enabled,
                autotext_running: autotext.running,
                autotext_events: autotext.events_seen,
                clipboard_enabled,
                hotkey_registry: Arc::new(Mutex::new(Vec::new())),
                current_app,
            });

            // Phase 2: register the popup hotkey + all per-phrase hotkeys.
            #[cfg(desktop)]
            {
                reregister_hotkeys(app.handle());
            }

            // M5: system tray (Show / Quit).
            let show = MenuItem::with_id(app, "show", "Show phrasexp", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("phrasexp")
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_folders,
            commands::create_folder,
            commands::get_folder,
            commands::delete_folder,
            commands::list_phrases,
            commands::get_phrase,
            commands::create_phrase,
            commands::update_phrase,
            commands::delete_phrase,
            commands::expand_phrase,
            commands::inject_phrase,
            commands::list_clipboard,
            commands::add_clipboard,
            commands::export_library,
            commands::import_csv,
            commands::find_duplicates,
            commands::list_versions,
            commands::restore_version,
            commands::autotext_status,
            commands::set_autotext,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
