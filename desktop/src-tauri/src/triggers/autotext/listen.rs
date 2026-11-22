//! The global key listener: key→char mapping, hotkey/chord parsing, the rdev/evdev
//! event loop (`start`), and echo-suppressed injection.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rdev::{listen, Event, EventType, Key};
use tauri::{Emitter, Manager};

use crate::db::Db;
use crate::triggers::Injector;

use super::expand::{
    fire_delayed, maybe_expand, quiet_clear, transition_pending, PendingAction, PendingMatch,
};
use super::index::AbbrevIndex;
use super::offer::{
    offer_latch_active, offer_latch_off, show_spellcheck_offer, CHORD_MODS_HELD, OFFER_POPUP_SHOWN,
};
use super::spellcheck::{current_word, maybe_correct, split_token};

const MAX_BUFFER: usize = 64;

fn is_modifier(k: Key) -> bool {
    matches!(
        k,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
    )
}

/// Press/release side of modifier tracking: fold `key` into the held-modifier flags.
fn track_modifier(
    k: Key,
    down: bool,
    shift: &mut bool,
    ctrl: &mut bool,
    alt: &mut bool,
    meta: &mut bool,
) {
    match k {
        Key::ShiftLeft | Key::ShiftRight => *shift = down,
        Key::ControlLeft | Key::ControlRight => *ctrl = down,
        Key::Alt | Key::AltGr => *alt = down,
        Key::MetaLeft | Key::MetaRight => *meta = down,
        _ => {}
    }
}

/// Map a physical key (when the offer popup is active) to a short name the frontend reacts to.
/// Shifted digits (symbols) are intentionally not routed — except "+", which is Shift+= and is
/// routed as the add-to-dictionary shortcut. Returns None for non-nav keys so the listener
/// continues normal processing for them.
///
/// While the X11 keyboard grab is active, routed keys are delivered only to the popup window,
/// so the target field never sees them. If the grab failed (best-effort: it retries then gives
/// up), a routed key double-delivers (the physical keystroke reaches the field too). That is
/// the existing behavior for Enter/Escape already, so Backspace is no worse.
fn offer_nav_key(key: Key, shift: bool) -> Option<&'static str> {
    use Key::*;
    match key {
        UpArrow => Some("up"),
        DownArrow => Some("down"),
        Return => Some("enter"),
        Escape => Some("escape"),
        Backspace => Some("backspace"),
        Equal if shift => Some("plus"),
        Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 if !shift => Some(match key {
            Num1 => "1",
            Num2 => "2",
            Num3 => "3",
            Num4 => "4",
            Num5 => "5",
            Num6 => "6",
            Num7 => "7",
            Num8 => "8",
            Num9 => "9",
            _ => unreachable!(),
        }),
        KeyD if !shift => Some("d"),
        _ => None,
    }
}

/// Map a hotkey token (e.g. "space", "f8", "a") to an rdev Key, for parsing the
/// configurable spellcheck toggle hotkey. Supports the keys a toggle realistically uses.
fn name_to_rdev_key(name: &str) -> Option<Key> {
    use Key::*;
    Some(match name {
        "space" => Space,
        "\\" => BackSlash,
        "," | "comma" => Comma,
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

/// An app-level global hotkey (popup / selection-AI / repeat-last / per-phrase),
/// resolved by the key listener instead of the global-shortcut plugin — that
/// plugin is X11-only, so on Wayland its grabs never fire for native-Wayland
/// windows (see `lib.rs::reregister_hotkeys`).
#[derive(Clone, Debug, PartialEq)]
pub struct AppChord {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: Key,
    pub token: String,
}

impl AppChord {
    /// Parse a "Control+Alt+A" style hotkey into a chord carrying `token`.
    pub fn parse(s: &str, token: &str) -> Option<AppChord> {
        let (shift, ctrl, alt, meta, key) = parse_chord(s)?;
        Some(AppChord {
            shift,
            ctrl,
            alt,
            meta,
            key,
            token: token.to_string(),
        })
    }
}

/// First chord matching the held modifiers + pressed key, if any. Pure so the
/// hotkey path is unit-testable without a listener.
pub fn match_app_chord(
    chords: &[AppChord],
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
    key: Key,
) -> Option<String> {
    chords
        .iter()
        .find(|c| {
            c.key == key && c.shift == shift && c.ctrl == ctrl && c.alt == alt && c.meta == meta
        })
        .map(|c| c.token.clone())
}

/// Parse a hotkey string like "Control+Shift+Space" into (shift, ctrl, alt, meta, key).
/// Returns None if the terminal key is missing or unrecognized.
fn parse_chord(s: &str) -> Option<(bool, bool, bool, bool, Key)> {
    let (mut sh, mut ct, mut al, mut me) = (false, false, false, false);
    let mut key = None;
    for raw in s.split('+') {
        let part = raw.trim().to_lowercase();
        match part.as_str() {
            "control" | "ctrl" => ct = true,
            "shift" => sh = true,
            "alt" | "option" | "opt" => al = true,
            "meta" | "super" | "win" | "cmd" | "command" => me = true,
            other => key = name_to_rdev_key(other),
        }
    }
    Some((sh, ct, al, me, key?))
}

fn clears_buffer(k: Key) -> bool {
    matches!(
        k,
        Key::UpArrow
            | Key::DownArrow
            | Key::LeftArrow
            | Key::RightArrow
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Delete
            | Key::Insert
            | Key::Return
            | Key::Tab
            | Key::Escape
            | Key::CapsLock
    )
}

/// True if the currently focused window matches an exclusion list (by title
/// substring, case-insensitive). Returns false when the list is empty (fast path) or
/// when the focused window can't be detected, so detection failures never silently
/// disable the feature. Used by BOTH exclusion lists (spellcheck + autotext).
fn title_excluded(
    current: &Mutex<Option<String>>,
    exclude: &Arc<Mutex<std::collections::HashSet<String>>>,
) -> bool {
    let exset = match exclude.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    if exset.is_empty() {
        return false;
    }
    current
        .lock()
        .map(|g| crate::triggers::app_context::title_matches_any(&g, &exset))
        .unwrap_or(false)
}

/// The just-completed sentence from the rolling buffer, if one is identifiable:
/// buffer ends with a sentence terminator and a PREVIOUS terminator (or newline)
/// exists inside the window so the sentence extent is fully known. The buffer
/// keeps only MAX_BUFFER chars — without an in-window start boundary we can't
/// know where the sentence begins, and backspacing would eat pre-window text.
/// Pure — unit-tested.
fn sentence_candidate(buffer: &str) -> Option<String> {
    let trimmed = buffer.trim_end();
    let last = trimmed.chars().last()?;
    if !matches!(last, '.' | '!' | '?') {
        return None;
    }
    let head = &trimmed[..trimmed.len() - last.len_utf8()];
    let start = head.rfind(['.', '!', '?', '\n'])?;
    let sentence = trimmed[start + 1..].trim();
    let n = sentence.chars().count();
    if !(8..=200).contains(&n) {
        return None; // not a sentence / too long for a quick review pass
    }
    Some(sentence.to_string())
}

/// Fire-and-forget AI grammar pass over a just-completed sentence. Shows the
/// offer popup only when AI is enabled AND the user hasn't typed anything since
/// the trigger — otherwise the correction's backspaces would eat their newer
/// text. Terminal on every error path (review must never disrupt typing).
fn spawn_sentence_review(
    sentence: String,
    db: std::sync::Arc<crate::db::Db>,
    app: tauri::AppHandle,
    offer: Arc<Mutex<Option<String>>>,
    candidates: Arc<Mutex<Vec<String>>>,
    typed: Arc<AtomicU64>,
    typed_at_trigger: u64,
) {
    tauri::async_runtime::spawn(async move {
        let settings = match db.load_settings() {
            Ok(s) => s,
            Err(_) => return,
        };
        // The setting sends typed text to the AI provider — never do that silently
        // when AI is off (it's off ⇒ review_sentence can't do anything anyway).
        if !settings.ai_enabled {
            return;
        }
        let prompt = format!(
            "Fix grammar and spelling in this sentence. Reply with only the corrected \
             sentence — no quotes, no commentary.\n\n{sentence}"
        );
        let Ok(raw) = crate::triggers::ai::complete(&settings, &prompt).await else {
            return;
        };
        let corrected = raw.trim().trim_matches('"').to_string();
        if corrected.is_empty() || corrected == sentence {
            return;
        }
        if typed.load(Ordering::Relaxed) != typed_at_trigger {
            return; // user typed on — don't backspace over their newer words
        }
        if let Ok(mut g) = offer.lock() {
            *g = Some(sentence.clone());
        }
        if let Ok(mut g) = candidates.lock() {
            *g = vec![corrected.clone()];
        }
        show_spellcheck_offer(&app, sentence, vec![corrected]);
    });
}

/// US-layout mapping of a physical key (+ shift state) to the typed character, if printable.
/// Unknown/unmapped keys return None (the caller treats that as "reset the buffer").
pub fn key_to_char(key: Key, shifted: bool) -> Option<char> {
    use Key::*;
    let c = match key {
        KeyA => 'a',
        KeyB => 'b',
        KeyC => 'c',
        KeyD => 'd',
        KeyE => 'e',
        KeyF => 'f',
        KeyG => 'g',
        KeyH => 'h',
        KeyI => 'i',
        KeyJ => 'j',
        KeyK => 'k',
        KeyL => 'l',
        KeyM => 'm',
        KeyN => 'n',
        KeyO => 'o',
        KeyP => 'p',
        KeyQ => 'q',
        KeyR => 'r',
        KeyS => 's',
        KeyT => 't',
        KeyU => 'u',
        KeyV => 'v',
        KeyW => 'w',
        KeyX => 'x',
        KeyY => 'y',
        KeyZ => 'z',
        Num0 => '0',
        Num1 => '1',
        Num2 => '2',
        Num3 => '3',
        Num4 => '4',
        Num5 => '5',
        Num6 => '6',
        Num7 => '7',
        Num8 => '8',
        Num9 => '9',
        Space => ' ',
        Comma => ',',
        Dot => '.',
        Slash => '/',
        SemiColon => ';',
        Minus => '-',
        Equal => '=',
        Quote => '\'',
        _ => return None,
    };
    Some(if shifted {
        match c {
            'a'..='z' => c.to_ascii_uppercase(),
            '1' => '!',
            '2' => '@',
            '3' => '#',
            '4' => '$',
            '5' => '%',
            '6' => '^',
            '7' => '&',
            '8' => '*',
            '9' => '(',
            '0' => ')',
            '-' => '_',
            '=' => '+',
            ',' => '<',
            '.' => '>',
            '/' => '?',
            ';' => ':',
            '\'' => '"',
            _ => c,
        }
    } else {
        c
    })
}

pub struct AutotextHandle {
    pub events_seen: Arc<AtomicU64>,
    pub running: Arc<AtomicBool>,
}

/// Spawn the global key listener. `enabled` is shared with the caller so toggling
/// it (via settings) takes effect live. Blocks in its own thread via rdev::listen.
#[allow(clippy::too_many_arguments)]
pub fn start(
    db: Arc<Db>,
    injector: Arc<dyn Injector>,
    index: Arc<Mutex<AbbrevIndex>>,
    enabled: Arc<AtomicBool>,
    spellcheck: Arc<AtomicBool>,
    case_adapt: Arc<AtomicBool>,
    // Opt-in sentence review: offer an AI grammar pass after sentence end.
    review_sentence: Arc<AtomicBool>,
    // Monotonic keystroke counter — the async review skips showing the popup
    // when the user typed anything between trigger and AI response.
    typed_counter: Arc<AtomicU64>,
    current: Arc<Mutex<Option<String>>>,
    ignore: Arc<Mutex<std::collections::HashSet<String>>>,
    mode: Arc<Mutex<String>>,
    learned: Arc<Mutex<HashMap<String, String>>>,
    spellcheck_hotkey: Arc<Mutex<String>>,
    exclude_apps: Arc<Mutex<std::collections::HashSet<String>>>,
    // Window-title substrings where expansion is fully disabled (global exclusion).
    autotext_exclude: Arc<Mutex<std::collections::HashSet<String>>>,
    delivery: Arc<Mutex<String>>,
    injecting: Arc<AtomicBool>,
    last_echo: Arc<Mutex<Option<std::time::Instant>>>,
    offer: Arc<Mutex<Option<String>>>,
    offer_candidates: Arc<Mutex<Vec<String>>>,
    buffer_clear: Arc<AtomicBool>,
    app_chords: Arc<Mutex<Vec<AppChord>>>,
    app: tauri::AppHandle,
) -> AutotextHandle {
    let events_seen = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(false));

    let en = enabled;
    let sc = spellcheck;
    let ca = case_adapt;
    let rv = review_sentence;
    let tctr = typed_counter;
    let seen = events_seen.clone();
    let run = running.clone();
    let idx = index.clone();
    let database = db.clone();
    let inj = injector.clone();
    let inj_flag = injecting.clone();
    let app_h = app.clone();
    let cur = current.clone();
    let ig = ignore.clone();
    let md = mode.clone();
    let lrn = learned.clone();
    let sk = spellcheck_hotkey.clone();
    let ex = exclude_apps.clone();
    let atx_ex = autotext_exclude.clone();
    let dl = delivery.clone();
    let ofr = offer.clone();
    let ocs = offer_candidates.clone();
    let bcl = buffer_clear.clone();
    let ach = app_chords;
    let le = last_echo;

    std::thread::spawn(move || {
        run.store(true, Ordering::Relaxed);
        let mut buffer = String::new();
        let mut shift = false;
        let mut ctrl = false;
        let mut alt = false;
        let mut meta = false;
        let mut last_correction: Option<(String, String)> = None; // (original, corrected)
        let mut correction_undone = false;
        // 1.4 delimiter mode: a matched delayed abbreviation parked until the next
        // key is (or isn't) a delimiter. Single-threaded listener — plain local.
        let mut pending: Option<PendingMatch> = None;

        let callback = move |event: Event| {
            seen.fetch_add(1, Ordering::Relaxed);
            // Skip our own injected keystrokes (enigo/XTest re-enters the listener), and
            // timestamp each echo so the injection worker knows when they've drained.
            if inj_flag.load(Ordering::SeqCst) {
                if let EventType::KeyPress(k) = event.event_type {
                    crate::debug_log(&format!("suppressed (injecting): {k:?}"));
                }
                if let Ok(mut t) = le.lock() {
                    *t = Some(std::time::Instant::now());
                }
                return;
            }
            match event.event_type {
                EventType::KeyPress(key) => {
                    crate::debug_log(&format!(
                        "key {key:?} shift={shift} src={}",
                        event.name.as_deref().unwrap_or("?")
                    ));
                    // Buffer-clear handshake: an accepted offer-mode correction rewrote the
                    // screen out from under this thread-local buffer; drop it so the
                    // abbreviation / correction logic resyncs on the next real key. (Echoed
                    // events returned early above.)
                    if bcl.swap(false, Ordering::SeqCst) {
                        crate::debug_log("buffer cleared (handshake)");
                        buffer.clear();
                    }
                    // Detect the configurable spellcheck toggle hotkey (settings.spellcheck_hotkey).
                    if shift || ctrl || alt || meta {
                        if let Some((s, c, a, m, trigger)) =
                            sk.lock().ok().and_then(|h| parse_chord(&h))
                        {
                            if key == trigger && shift == s && ctrl == c && alt == a && meta == m {
                                let new_val = !sc.load(Ordering::Relaxed);
                                sc.store(new_val, Ordering::Relaxed);
                                let _ = app_h.emit("spellcheck-toggled", new_val);
                                return;
                            }
                        }
                    }
                    // App-level hotkeys (popup / selection-AI / repeat / per-phrase).
                    // Runs before the autotext/spellcheck gate so the chords work even
                    // with every typing module off.
                    if let Some(token) = ach
                        .lock()
                        .ok()
                        .and_then(|g| match_app_chord(&g, shift, ctrl, alt, meta, key))
                    {
                        crate::debug_log(&format!("app chord fired: {token}"));
                        // A pending spellcheck offer must not outlive this: its
                        // expiry cleanup hides the shared popup window, which
                        // would yank the selection-AI / search popup right after
                        // show_popup() renders it (observed live).
                        offer_latch_off();
                        if shift || ctrl || alt || meta {
                            CHORD_MODS_HELD.store(true, Ordering::SeqCst);
                        }
                        let h = app_h.clone();
                        let hc = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            crate::dispatch_app_token(&hc, &token);
                        });
                        return;
                    }
                    // Track held modifiers precisely (for the toggle hotkey above).
                    track_modifier(key, true, &mut shift, &mut ctrl, &mut alt, &mut meta);
                    if is_modifier(key) {
                        return;
                    }
                    // Offer popup up? Route its nav keys via event. rdev observes keys globally
                    // regardless of focus, so this works even when X focus/grab don't deliver to
                    // the webview DOM. The keyboard grab (if active) keeps these keys from also
                    // reaching the text app.
                    if offer_latch_active() {
                        pending = None;
                        if let Some(name) = offer_nav_key(key, shift) {
                            crate::debug_log(&format!("offer-nav routed: {name}"));
                            let _ = app_h.emit("offer-key", name);
                            return;
                        }
                    } else if OFFER_POPUP_SHOWN.swap(false, Ordering::SeqCst) {
                        // Latch expired (or was released) without the webview dismissing
                        // itself — hide the window here so a dead popup can't linger and
                        // keep implying keys are captured.
                        crate::debug_log("offer latch ended — hiding popup");
                        let h = app_h.clone();
                        let _ = h.run_on_main_thread({
                            let h = h.clone();
                            move || {
                                if let Some(w) = h.get_webview_window("popup") {
                                    let _ = w.hide();
                                }
                            }
                        });
                    }
                    // Modules are independent from here: shared buffer/pending bookkeeping
                    // runs whenever either module is live; expansion fires only under en
                    // (guards below), correction only under sc (its own gate on the space
                    // branch). The early-out keeps all-off keystrokes near-free.
                    let en_live = en.load(Ordering::Relaxed);
                    if !en_live && !sc.load(Ordering::Relaxed) {
                        return;
                    }
                    if ctrl || alt || meta {
                        return; // part of a keyboard shortcut, not typing
                    }
                    // Tier 1D: undo the last spellcheck correction on the first Left arrow after it.
                    // (Backspace is left as normal editing.)
                    if key == Key::LeftArrow {
                        if let Some((ref orig, ref corrected)) = last_correction {
                            let erase_count = corrected.chars().count() + 1;
                            let orig_owned = orig.clone();
                            // Learn: add to ignore list so it won't be corrected again.
                            if let Ok(mut guard) = ig.lock() {
                                guard.insert(orig.to_lowercase());
                            }
                            // Unlearn the typo→correction pair: the undo means it was wrong.
                            let orig_lower = orig.to_lowercase();
                            if let Ok(mut g) = lrn.lock() {
                                g.remove(&orig_lower);
                            }
                            let _ = database.remove_spellcheck_learned(&orig_lower);
                            let _ = app_h.emit("spellcheck-learned", orig.clone());
                            // Update buffer to reflect the undo.
                            let erase_len = corrected.len() + 1;
                            if buffer.len() >= erase_len {
                                buffer.truncate(buffer.len() - erase_len);
                                buffer.push_str(orig);
                                buffer.push(' ');
                            }
                            last_correction = None;
                            correction_undone = false;
                            // Inject (erase corrected + retype original) off-thread with echo suppression.
                            inject_with_suppression(&app_h, &inj_flag, &le, move |inj| {
                                crate::triggers::inject::log_if_err(
                                    inj.backspace(erase_count),
                                    "undo correction erase",
                                );
                                crate::triggers::inject::log_if_err(
                                    inj.type_text(&format!("{orig_owned} ")),
                                    "undo correction retype",
                                );
                            });
                            return;
                        }
                        // No pending correction: fall through — Left arrow clears the buffer below.
                    }
                    if key == Key::Backspace {
                        buffer.pop();
                        pending = None; // 1.4: editing cancels a parked abbreviation.
                                        // A backspace after a correction counts as rejecting it.
                        if last_correction.is_some() {
                            correction_undone = true;
                        }
                        return;
                    }
                    // 1.4: Return/Tab fire a pending delayed abbreviation (they're
                    // delimiters). Other buffer-clearing keys cancel it.
                    if pending.is_some() && matches!(key, Key::Return | Key::Tab) {
                        let p = pending.take().unwrap();
                        if en_live && !title_excluded(&cur, &atx_ex) && buffer.ends_with(&p.abbr) {
                            fire_delayed(
                                &p,
                                None,
                                &mut buffer,
                                &database,
                                &inj_flag,
                                &le,
                                &ca,
                                &cur,
                                &app_h,
                            );
                            return; // consumed — never reaches clears_buffer/spellcheck
                        }
                        pending = None;
                    }
                    if clears_buffer(key) {
                        buffer.clear();
                        pending = None;
                        return;
                    }
                    if let Some(ch) = key_to_char(key, shift) {
                        // 1.4: a parked delayed abbreviation decides its fate on this char.
                        if let Some(p) = pending.take() {
                            match transition_pending(&p, ch, &buffer) {
                                PendingAction::Fire(delim)
                                    if en_live && !title_excluded(&cur, &atx_ex) =>
                                {
                                    fire_delayed(
                                        &p,
                                        Some(delim),
                                        &mut buffer,
                                        &database,
                                        &inj_flag,
                                        &le,
                                        &ca,
                                        &cur,
                                        &app_h,
                                    );
                                    return; // delimiter consumed; spellcheck block skipped
                                }
                                _ => {
                                    // Drop — or a fire the expander no longer owns
                                    // (turned off mid-park): the char is ordinary input.
                                }
                            }
                        }
                        buffer.push(ch);
                        tctr.fetch_add(1, Ordering::Relaxed);
                        if buffer.len() > MAX_BUFFER {
                            let drop_n = buffer.len() - MAX_BUFFER;
                            buffer.drain(0..drop_n);
                            pending = None; // truncation can invalidate the parked suffix
                        }
                        // Expansion (delayed park + immediate fire) belongs to the
                        // expander alone; the spellchecker only needs the buffer.
                        if en_live && !title_excluded(&cur, &atx_ex) {
                            // Park a delayed match instead of firing it immediately.
                            if let Some((abbr, ids)) =
                                idx.lock().ok().and_then(|i| i.match_delayed(&buffer))
                            {
                                pending = Some(PendingMatch { abbr, ids });
                            }
                            maybe_expand(
                                &mut buffer,
                                &idx,
                                &database,
                                &*inj,
                                &inj_flag,
                                &ca,
                                &cur,
                                &le,
                                &app_h,
                            );
                        }
                        if ch == ' '
                            && !buffer.is_empty()
                            && sc.load(Ordering::Relaxed)
                            && !title_excluded(&cur, &ex)
                        {
                            let delivery_mode = dl
                                .lock()
                                .ok()
                                .map(|g| g.clone())
                                .unwrap_or_else(|| "silent".to_string());
                            if delivery_mode == "offer" {
                                // Offer mode: surface candidates via a popup; never
                                // silent-correct, so the cursor/undo race can't occur.
                                // Only fuzzy corrections are offered (capitalization fixes
                                // are unambiguous and stay silent-or-nothing).
                                let (word_start, raw) = current_word(&buffer);
                                let split = split_token(&raw);
                                if let Some((_, core, _)) = split {
                                    if core.chars().count() >= 3 {
                                        let ig_set =
                                            ig.lock().ok().map(|g| g.clone()).unwrap_or_default();
                                        let mode_str = md
                                            .lock()
                                            .ok()
                                            .map(|m| m.clone())
                                            .unwrap_or_else(|| "normal".to_string());
                                        let prev = buffer[..word_start]
                                            .trim_end()
                                            .rsplit(' ')
                                            .next()
                                            .map(|p| p.to_string())
                                            .filter(|p| !p.is_empty());
                                        let cands = idx
                                            .lock()
                                            .ok()
                                            .map(|i| {
                                                i.rank_candidates(
                                                    core,
                                                    prev.as_deref(),
                                                    &ig_set,
                                                    &mode_str,
                                                    4,
                                                )
                                            })
                                            .unwrap_or_default();
                                        if !cands.is_empty() {
                                            // Store the raw token (incl. wrappers) so the
                                            // injector erases the full thing; surface the clean
                                            // core to the popup (display + "add to dictionary").
                                            if let Ok(mut g) = ofr.lock() {
                                                *g = Some(raw.clone());
                                            }
                                            if let Ok(mut g) = ocs.lock() {
                                                *g = cands.clone();
                                            }
                                            show_spellcheck_offer(&app_h, core.to_string(), cands);
                                        }
                                    }
                                }
                            } else {
                                let corrected = maybe_correct(
                                    &mut buffer,
                                    &idx,
                                    &inj_flag,
                                    &le,
                                    &ig,
                                    &md,
                                    &lrn,
                                    &database,
                                    &app_h,
                                );
                                if let Some((orig, corrected_word)) = corrected {
                                    // Store (original, corrected) for undo support.
                                    last_correction = Some((orig, corrected_word));
                                    correction_undone = false;
                                } else if correction_undone {
                                    // User backspaced after a correction, then typed a new word.
                                    // If they re-typed the same misspelled word, learn to ignore it.
                                    let (_, raw) = current_word(&buffer);
                                    let word = split_token(&raw).map(|(_, c, _)| c).unwrap_or("");
                                    if let Some((ref orig, _)) = last_correction {
                                        if word.to_lowercase() == orig.to_lowercase() {
                                            if let Ok(mut guard) = ig.lock() {
                                                guard.insert(orig.to_lowercase());
                                            }
                                            let _ = app_h.emit("spellcheck-learned", orig.clone());
                                        }
                                    }
                                    last_correction = None;
                                    correction_undone = false;
                                }
                            }
                        }
                        // Opt-in "review sentence" (independent of spellcheck): after a
                        // sentence-ending delimiter + space, send the sentence through an
                        // AI grammar pass and OFFER the correction — loud popup, explicit
                        // Accept, off by default (scope doc A2). Same app exclusions as
                        // spellcheck; the keystroke-race guard lives in the spawned task.
                        if ch == ' ' && rv.load(Ordering::Relaxed) && !title_excluded(&cur, &ex) {
                            if let Some(sentence) = sentence_candidate(&buffer) {
                                spawn_sentence_review(
                                    sentence,
                                    database.clone(),
                                    app_h.clone(),
                                    ofr.clone(),
                                    ocs.clone(),
                                    tctr.clone(),
                                    tctr.load(Ordering::Relaxed),
                                );
                            }
                        }
                    } else {
                        // Unmapped key: the buffer is no longer a reliable suffix.
                        buffer.clear();
                    }
                }
                EventType::KeyRelease(key) => {
                    track_modifier(key, false, &mut shift, &mut ctrl, &mut alt, &mut meta)
                }
                _ => {}
            }
            // All chord modifiers physically released → capture can copy cleanly.
            if matches!(event.event_type, EventType::KeyRelease(k) if is_modifier(k))
                && !shift
                && !ctrl
                && !alt
                && !meta
            {
                CHORD_MODS_HELD.store(false, Ordering::SeqCst);
            }
        };
        // Event source: evdev (/dev/input/event* — the only source that sees native
        // Wayland apps; rdev's listen() is XRecord-only, i.e. XWayland apps). Falls back
        // to rdev's XRecord listen when no evdev device is readable (X11 without the
        // `input` group).
        let shared: Arc<Mutex<dyn FnMut(Event) + Send>> = Arc::new(Mutex::new(callback));
        #[cfg(target_os = "linux")]
        let evdev_ok = crate::triggers::evdev_listen::listen(Arc::clone(&shared));
        #[cfg(not(target_os = "linux"))]
        let evdev_ok = false;
        if !evdev_ok {
            let cb = shared;
            if let Err(e) = listen(move |e| (cb.lock().unwrap_or_else(|p| p.into_inner()))(e)) {
                eprintln!("[phrasexp] rdev listen error: {e:?}");
            }
        }
        run.store(false, Ordering::Relaxed);
    });

    AutotextHandle {
        events_seen,
        running,
    }
}

/// Run an injection on a worker thread with echo suppression. Sets the flag, resets the
/// echo timer, spawns a thread that runs `inject`, then waits for the echoed keystrokes
/// (XRecord) to arrive and go quiet before clearing the flag. This keeps the listener
/// thread free (no blocking/lag) and prevents echoes from re-entering the buffer.
pub(crate) fn inject_with_suppression(
    app: &tauri::AppHandle,
    inj_flag: &Arc<AtomicBool>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
    inject: impl FnOnce(&dyn Injector) + Send + 'static,
) {
    inj_flag.store(true, Ordering::SeqCst);
    if let Ok(mut t) = last_echo.lock() {
        *t = None;
    }
    let state = app.state::<crate::AppState>();
    let inj_arc = state.injector.clone();
    let flag = inj_flag.clone();
    let le = last_echo.clone();
    std::thread::spawn(move || {
        inject(&*inj_arc);
        quiet_clear(&flag, &le);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdev::Key;

    // App hotkey chords: parse + listener match. The delivery half (which thread
    // fires, where) is OS-bound and covered by manual smoke testing instead.

    #[test]
    fn app_chord_parse_and_match() {
        let c = AppChord::parse("Control+Alt+A", "__selection_ai__").expect("parses");
        assert_eq!(c.key, Key::KeyA);
        assert!((c.ctrl, c.alt, c.shift, c.meta) == (true, true, false, false));
        // Exact chord matches; plain A or Ctrl+A must not.
        assert_eq!(
            match_app_chord(
                std::slice::from_ref(&c),
                false,
                true,
                true,
                false,
                Key::KeyA
            ),
            Some("__selection_ai__".into())
        );
        assert_eq!(
            match_app_chord(
                std::slice::from_ref(&c),
                false,
                true,
                false,
                false,
                Key::KeyA
            ),
            None
        );
    }

    #[test]
    fn app_chord_bare_key_and_wrong_key() {
        let c = AppChord::parse("F8", "phrase-1").expect("parses");
        assert_eq!(
            match_app_chord(&[c], false, false, false, false, Key::F8),
            Some("phrase-1".into())
        );
        let c2 = AppChord::parse("Control+Alt+A", "x").unwrap();
        assert_eq!(
            match_app_chord(&[c2], false, true, true, false, Key::KeyB),
            None
        );
    }

    #[test]
    fn app_chord_rejects_garbage() {
        assert!(AppChord::parse("Control+Alt+", "t").is_none());
        assert!(AppChord::parse("Control+Alt+PrintScreen", "t").is_none());
    }

    #[test]
    fn app_chord_punctuation_keys() {
        // Punctuation terminals: literal char or name, e.g. "Control+Super+,".
        for s in ["Control+Super+,", "Control+Super+comma", "Control+Super+\\"] {
            let c = AppChord::parse(s, "t").unwrap_or_else(|| panic!("parses: {s}"));
            assert!((c.ctrl, c.meta, c.alt) == (true, true, false), "{s}");
        }
    }

    #[test]
    fn key_mapping_letters_digits_shift() {
        assert_eq!(key_to_char(Key::KeyA, false), Some('a'));
        assert_eq!(key_to_char(Key::KeyA, true), Some('A'));
        assert_eq!(key_to_char(Key::Num5, false), Some('5'));
        assert_eq!(key_to_char(Key::Num5, true), Some('%'));
        assert_eq!(key_to_char(Key::Space, false), Some(' '));
        assert_eq!(key_to_char(Key::Comma, true), Some('<'));
    }

    #[test]
    fn unmapped_key_returns_none() {
        assert_eq!(key_to_char(Key::F1, false), None);
    }

    #[test]
    fn parse_chord_default_combo() {
        // "Control+Shift+Space" → shift+ctrl held, terminal key Space.
        let (sh, ct, al, me, key) = parse_chord("Control+Shift+Space").unwrap();
        assert!(sh && ct && !al && !me);
        assert_eq!(key, Key::Space);
    }

    #[test]
    fn parse_chord_aliases_and_whitespace() {
        // "ctrl + alt + f8" — aliases + surrounding spaces + a function key.
        let (sh, ct, al, me, key) = parse_chord("ctrl + alt + f8").unwrap();
        assert!(!sh && ct && al && !me);
        assert_eq!(key, Key::F8);
    }

    #[test]
    fn parse_chord_rejects_unknown_key() {
        assert!(parse_chord("Control+Shift+Unknown").is_none());
        assert!(parse_chord("Control+Shift").is_none()); // no terminal key
    }

    #[test]
    fn sentence_candidate_finds_last_sentence() {
        assert_eq!(
            sentence_candidate("Hello world, this is fine. I am tireD now. ").as_deref(),
            Some("I am tireD now.")
        );
        assert_eq!(
            sentence_candidate("First! Second one here? ").as_deref(),
            Some("Second one here?")
        );
        // Newline also bounds a sentence.
        assert_eq!(
            sentence_candidate("Subject line\nThis has a typo here. ").as_deref(),
            Some("This has a typo here.")
        );
    }

    #[test]
    fn sentence_candidate_rejects_non_sentences() {
        // No terminator at the end (buffer mid-word / just a space).
        assert_eq!(sentence_candidate("typing a sent"), None);
        // No start boundary in the window — extent unknown (truncated buffer).
        assert_eq!(sentence_candidate("and this keeps going on and on. "), None);
        // Too short to be a sentence.
        assert_eq!(sentence_candidate("One. Ok. "), None);
        assert_eq!(sentence_candidate(""), None);
    }
}
