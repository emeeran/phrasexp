//! Spellcheck-offer popup flow: the nav-key latch statics, the `spellcheck-offer`
//! event payload, X11 focus/keyboard-grab helpers, and the accept/learn/delete
//! actions the popup commands call back into.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::triggers::Injector;

use super::spellcheck::split_token;

/// Nav-key routing is active while this latch is set. Dismissal normally comes from the
/// popup's own `release_keyboard` invoke, but if the webview died (failed page load, crash)
/// nothing would ever clear it and the popup would eat Enter/Backspace/digits system-wide
/// forever. So it stores an expiry timestamp, not a bool: routing self-cancels after
/// `OFFER_LATCH_TTL` no matter what the frontend does.
/// Single-instance app, so statics are sufficient.
pub static OFFER_LATCH_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
/// Set when the latch turns on; consumed once to hide the window from the listener thread if
/// the webview never dismisses itself (dead page can't call `release_keyboard`/hide).
pub static OFFER_POPUP_SHOWN: AtomicBool = AtomicBool::new(false);

/// True while the user's fingers are still holding the modifiers of a chord that
/// just fired an app hotkey. The selection-AI capture waits on this before its
/// synthetic copy — a Ctrl+C sent under held Alt lands as Alt+Ctrl+C, which most
/// apps don't treat as copy, so the capture would silently come up empty.
pub static CHORD_MODS_HELD: AtomicBool = AtomicBool::new(false);
pub const OFFER_LATCH_TTL: std::time::Duration = std::time::Duration::from_secs(15);

fn latch_on_for(ttl: std::time::Duration) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    OFFER_LATCH_UNTIL_MS.store((now + ttl).as_millis() as u64, Ordering::SeqCst);
    OFFER_POPUP_SHOWN.store(true, Ordering::SeqCst);
}

/// Arm nav-key routing for this offer popup.
pub fn offer_latch_on() {
    latch_on_for(OFFER_LATCH_TTL);
}

/// Disarm routing (popup dismissed by the frontend or expired).
pub fn offer_latch_off() {
    OFFER_LATCH_UNTIL_MS.store(0, Ordering::SeqCst);
    OFFER_POPUP_SHOWN.store(false, Ordering::SeqCst);
}

pub fn offer_latch_active() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    OFFER_LATCH_UNTIL_MS.load(Ordering::SeqCst) > now.as_millis() as u64
}

/// Payload for the `spellcheck-offer` event: the original word + ranked candidates.
#[derive(Clone, serde::Serialize)]
struct SpellcheckOffer {
    original: String,
    candidates: Vec<String>,
}

// On X11 the popup is triggered by the rdev key listener (a passive observer, not an
// OS-registered hotkey), so `Window::set_focus()` is usually denied as focus-stealing and the
// popup never receives keyboard input. Two direct X-level mechanisms fix this:
//   * `set_focus` — XRaiseWindow + XSetInputFocus (bypasses WM focus policy).
//   * `grab_keyboard` — XGrabKeyboard, an ACTIVE grab: every key event routes to the popup at
//     the server level, independent of focus, until XUngrabKeyboard or the window is unmapped.
//     This is bulletproof — nothing else can receive keys while the popup is up. The pointer
//     is not grabbed, so the mouse can still click popup buttons (an escape hatch); and X
//     auto-releases the grab when the window hides (unmap), so there is no lockout risk.
// The Display* is reused from the window's own X connection (no XOpenDisplay needed).
#[cfg(target_os = "linux")]
mod x11_focus {
    use std::os::raw::{c_int, c_ulong, c_void};
    #[link(name = "X11")]
    extern "C" {
        fn XRaiseWindow(display: *mut c_void, w: c_ulong) -> c_int;
        fn XSetInputFocus(
            display: *mut c_void,
            focus: c_ulong,
            revert_to: c_int,
            time: c_ulong,
        ) -> c_int;
        fn XGrabKeyboard(
            display: *mut c_void,
            grab_window: c_ulong,
            owner_events: c_int,
            pointer_mode: c_int,
            keyboard_mode: c_int,
            time: c_ulong,
        ) -> c_int;
        fn XUngrabKeyboard(display: *mut c_void, time: c_ulong) -> c_int;
        fn XFlush(display: *mut c_void) -> c_int;
    }
    const REVERT_TO_PARENT: c_int = 0;
    const GRAB_MODE_ASYNC: c_int = 1; // don't freeze the device
    const OWNER_EVENTS_TRUE: c_int = 1; // deliver events normally to the webview
    const CURRENT_TIME: c_ulong = 0;
    const GRAB_SUCCESS: c_int = 0;

    /// Direct focus set (bypasses WM focus-stealing prevention).
    pub(crate) unsafe fn set_focus(display: *mut c_void, window: c_ulong) {
        XRaiseWindow(display, window);
        XSetInputFocus(display, window, REVERT_TO_PARENT, CURRENT_TIME);
        XFlush(display);
    }

    /// Active keyboard grab. Returns true on GrabSuccess; fails (returns false) if the window
    /// isn't viewable yet or another client already holds the keyboard. Best-effort.
    pub(crate) unsafe fn grab_keyboard(display: *mut c_void, window: c_ulong) -> bool {
        XGrabKeyboard(
            display,
            window,
            OWNER_EVENTS_TRUE,
            GRAB_MODE_ASYNC,
            GRAB_MODE_ASYNC,
            CURRENT_TIME,
        ) == GRAB_SUCCESS
    }

    pub(crate) unsafe fn ungrab_keyboard(display: *mut c_void) {
        XUngrabKeyboard(display, CURRENT_TIME);
        XFlush(display);
    }
}

/// Extract the (Display*, XID) for the window's X connection. raw-window-handle 0.6 splits the
/// XID (window handle) from the Display* (display handle).
#[cfg(target_os = "linux")]
fn x11_handles(
    win: &tauri::WebviewWindow,
) -> Option<(*mut std::ffi::c_void, std::os::raw::c_ulong)> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let window = win.window_handle().ok()?.as_raw();
    let display = win.display_handle().ok()?.as_raw();
    match (window, display) {
        (RawWindowHandle::Xlib(xw), RawDisplayHandle::Xlib(xd)) => xd
            .display
            .map(std::ptr::NonNull::as_ptr)
            .map(|d| (d, xw.window as std::os::raw::c_ulong)),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
pub fn force_focus_x11(win: &tauri::WebviewWindow) {
    if let Some((display, window)) = x11_handles(win) {
        unsafe { x11_focus::set_focus(display, window) };
    }
}

#[cfg(target_os = "linux")]
pub fn grab_keyboard_x11(win: &tauri::WebviewWindow) -> bool {
    x11_handles(win)
        .map(|(display, window)| unsafe { x11_focus::grab_keyboard(display, window) })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn ungrab_keyboard_x11(win: &tauri::WebviewWindow) {
    if let Some((display, _)) = x11_handles(win) {
        unsafe { x11_focus::ungrab_keyboard(display) };
    }
}

#[cfg(not(target_os = "linux"))]
pub fn force_focus_x11(_win: &tauri::WebviewWindow) {}
#[cfg(not(target_os = "linux"))]
pub fn grab_keyboard_x11(_win: &tauri::WebviewWindow) -> bool {
    false
}
#[cfg(not(target_os = "linux"))]
pub fn ungrab_keyboard_x11(_win: &tauri::WebviewWindow) {}

/// Windows: the foreground lock denies `set_focus` (SetForegroundWindow) when another
/// app owns foreground and this process hasn't received input recently — the popup then
/// shows without ever getting keys. The standard nudge is a synthetic tap+release of a
/// harmless modifier (Alt), which marks this process as recently-input and lets the next
/// SetForegroundWindow through. Same trick PowerToys Run and launcher-class apps use.
#[cfg(target_os = "windows")]
pub fn force_focus_win() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
    // SAFETY: two-argument syscall-style call; no state to misuse.
    unsafe {
        keybd_event(VK_MENU as u8, 0, 0, 0);
        keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

/// Offer-mode: show the shared popup near the cursor with correction candidates. No-op if
/// the popup is already visible (don't clobber a search session or a prior offer).
pub(super) fn show_spellcheck_offer(
    app: &tauri::AppHandle,
    original: String,
    candidates: Vec<String>,
) {
    crate::debug_log(&format!("offer {original:?} → {candidates:?}"));
    use tauri::{Emitter, LogicalSize, Manager, Size};
    if let Some(win) = app.get_webview_window("popup") {
        let visible = win.is_visible().unwrap_or(false);
        // Don't clobber an active search/chooser session. But if a prior OFFER is still
        // showing (OFFER_POPUP_ACTIVE), replace it — otherwise a 2nd typo while the first
        // offer is on screen would silently no-op, looking like "the popup stopped appearing".
        if visible && !offer_latch_active() {
            return;
        }
        // (Re)size the window to this offer's content: width from the longest word, height
        // from the candidate count. Done before show() so it never flashes the old size.
        let count = candidates.len().min(4) as f64;
        let longest = std::iter::once(original.chars().count())
            .chain(candidates.iter().map(|c| c.chars().count()))
            .max()
            .unwrap_or(0) as f64;
        let width = (280.0 + (longest - 9.0).max(0.0) * 7.0).clamp(280.0, 380.0);
        let height = (108.0 + count * 30.0).clamp(140.0, 240.0);
        let _ = win.set_size(Size::Logical(LogicalSize::new(width, height)));
        crate::position_popup_at_cursor(&win);
        if !visible {
            crate::summon_popup(app, &win);
        }
        // Keyboard focus + grab are taken post-render on the main thread by the `focus_popup`
        // / `grab_keyboard` commands (called from onOffer). Doing raw X here (listener thread)
        // races GTK's X access, so it's avoided.
    }
    offer_latch_on();
    let _ = app.emit(
        "spellcheck-offer",
        SpellcheckOffer {
            original,
            candidates,
        },
    );
}

/// Normalized (typo, correction) pair for learning from an accepted offer. `orig` is the
/// raw offered token (may carry punctuation wrappers, e.g. `"(teh,"`); the learned map
/// keys on the clean core so future occurrences match regardless of wrappers. Returns
/// None when the "correction" is a no-op (case-only) — nothing to learn.
pub fn learn_pair_from_offer(orig: &str, corrected: &str) -> Option<(String, String)> {
    // Single-word offers only: the opt-in sentence review's offers must not poison
    // the typo map with word→sentence entries.
    if orig.trim().chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let (_, core, _) = split_token(orig)?;
    let typo = core.to_lowercase();
    let fixed = corrected.to_lowercase();
    (typo != fixed).then_some((typo, fixed))
}

/// Validate and consume the pending offer: returns the original word iff `corrected` is one
/// of the candidates we emitted. This gate prevents a rogue frontend from driving an
/// arbitrary `backspace(N)` count via the apply-correction command.
pub fn take_pending_offer(
    corrected: &str,
    offer: &Arc<Mutex<Option<String>>>,
    offer_candidates: &Arc<Mutex<Vec<String>>>,
) -> Option<String> {
    let valid = offer_candidates
        .lock()
        .map(|g| g.iter().any(|c| c == corrected))
        .unwrap_or(false);
    if !valid {
        return None;
    }
    offer.lock().ok().and_then(|g| g.clone())
}

/// Characters to erase to remove a token (with its punctuation wrappers) and the
/// trailing space the offer flow types after every word. Shared by the correction
/// and delete-word injections and by the buffer-rewrite path in spellcheck.
pub(super) fn erase_len(lead: &str, core: &str, trailing: &str) -> usize {
    lead.chars().count() + core.chars().count() + trailing.chars().count() + 1 // + space
}

/// Replace `orig` (the offered word) with `corrected`: erase the word + its trailing space,
/// then type the corrected word + trailing space. The caller wraps this in echo-suppression.
/// Pure w.r.t. the injector so the backspace/type math is unit-testable with a mock.
pub fn inject_correction(orig: &str, corrected: &str, inj: &dyn Injector) {
    // `orig` may carry punctuation wrappers that were stripped for lookup (e.g. `recieve,`
    // or `(teh`); preserve them in the replacement so they survive the correction.
    let (lead, core, trailing) = split_token(orig).unwrap_or(("", orig, ""));
    super::super::inject::log_if_err(
        inj.backspace(erase_len(lead, core, trailing)),
        "correction erase",
    );
    super::super::inject::log_if_err(
        inj.type_text(&format!("{lead}{corrected}{trailing} ")),
        "correction type",
    );
}

/// Offer-mode "delete typed word" (Backspace while the popup is open): erase the offered word
/// and its trailing space, leaving the field as if the word was never typed. Same erase math as
/// `inject_correction`, just without the retype. Pure w.r.t. the injector so it's unit-testable.
pub fn inject_delete_word(orig: &str, inj: &dyn Injector) {
    let (lead, core, trailing) = split_token(orig).unwrap_or(("", orig, ""));
    super::super::inject::log_if_err(
        inj.backspace(erase_len(lead, core, trailing)),
        "delete-word erase",
    );
}

/// Surface the multi-match chooser: show the popup near the cursor + emit candidate ids.
pub(super) fn show_chooser(app: &tauri::AppHandle, ids: Vec<String>) {
    crate::debug_log(&format!("chooser → {} phrase(s)", ids.len()));
    use tauri::{Emitter, LogicalSize, Manager, Size};
    if let Some(win) = app.get_webview_window("popup") {
        // A visible popup (search session, AI flow) stays where it is — same rule as
        // show_popup. Cold summons appear near the cursor.
        if !win.is_visible().unwrap_or(false) {
            // Restore full size: a prior compact spellcheck offer may have shrunk it.
            let _ = win.set_size(Size::Logical(LogicalSize::new(560.0, 420.0)));
            crate::position_popup_at_cursor(&win);
        }
        // The chooser can fire cold (first typo with multiple matches) — summon_popup
        // activates the app where macOS needs it.
        crate::summon_popup(app, &win);
        // Focus is grabbed post-render by `focus_popup` (main thread) — see show_spellcheck_offer.
    }
    let _ = app.emit("autotext-choose", ids);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learn_pair_from_offer_strips_wrappers() {
        // Offered raw token includes wrappers; the learned key must be the clean core.
        assert_eq!(
            learn_pair_from_offer("(teh,", "the"),
            Some(("teh".into(), "the".into()))
        );
        assert_eq!(
            learn_pair_from_offer("\"recieve\"", "receive"),
            Some(("recieve".into(), "receive".into()))
        );
        // A real correction with case applied still learns the lowercase pair.
        assert_eq!(
            learn_pair_from_offer("teh", "The"),
            Some(("teh".into(), "the".into()))
        );
        // Case-only "correction" (same word lowercased) → nothing to learn.
        assert_eq!(learn_pair_from_offer("hello", "Hello"), None);
    }

    #[test]
    fn inject_correction_counts_word_plus_trailing_space() {
        let inj = crate::triggers::inject::test_injector::RecordingInjector::new();
        // "teh" is 3 chars + 1 trailing space → backspace(4), then type the replacement.
        inject_correction("teh", "the", &inj);
        assert_eq!(inj.snapshot(), vec!["backspace(4)", "type_text(\"the \")"]);
    }

    #[test]
    fn inject_correction_preserves_leading_punct() {
        let inj = crate::triggers::inject::test_injector::RecordingInjector::new();
        // "(teh" → erase paren(1)+core(3)+space(1)=5, retype "(the ".
        inject_correction("(teh", "the", &inj);
        assert_eq!(inj.snapshot(), vec!["backspace(5)", "type_text(\"(the \")"]);
    }

    #[test]
    fn inject_correction_preserves_trailing_punct() {
        let inj = crate::triggers::inject::test_injector::RecordingInjector::new();
        // "recieve," → erase core(7)+comma(1)+space(1)=9, type "receive, ".
        inject_correction("recieve,", "receive", &inj);
        assert_eq!(
            inj.snapshot(),
            vec!["backspace(9)", "type_text(\"receive, \")"]
        );
    }

    #[test]
    fn inject_delete_word_erases_word_plus_trailing_space() {
        let inj = crate::triggers::inject::test_injector::RecordingInjector::new();
        // "teh" is 3 chars + 1 trailing space → backspace(4), no retype.
        inject_delete_word("teh", &inj);
        assert_eq!(inj.snapshot(), vec!["backspace(4)"]);
    }

    #[test]
    fn inject_delete_word_includes_trailing_punct() {
        let inj = crate::triggers::inject::test_injector::RecordingInjector::new();
        // "recieve," → erase core(7)+comma(1)+space(1)=9, no retype.
        inject_delete_word("recieve,", &inj);
        assert_eq!(inj.snapshot(), vec!["backspace(9)"]);
    }

    #[test]
    fn take_pending_offer_validates_against_emitted_candidates() {
        let offer: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some("teh".into())));
        let candidates: Arc<Mutex<Vec<String>>> =
            Arc::new(Mutex::new(vec!["the".to_string(), "tea".to_string()]));
        // valid candidate → original returned, offer untouched
        assert_eq!(
            take_pending_offer("the", &offer, &candidates),
            Some("teh".to_string())
        );
        assert_eq!(offer.lock().unwrap().clone(), Some("teh".to_string()));
        // unknown candidate (would-be rogue frontend) → None, offer untouched
        assert_eq!(take_pending_offer("EVIL", &offer, &candidates), None);
        // no pending offer → None
        let no_offer: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        assert_eq!(take_pending_offer("the", &no_offer, &candidates), None);
    }

    #[test]
    fn offer_latch_expires_without_the_webview() {
        // The latch must self-cancel: a dead popup webview can never call
        // release_keyboard, so expiry is what stops it eating keys forever.
        latch_on_for(std::time::Duration::from_millis(10));
        assert!(offer_latch_active());
        assert!(OFFER_POPUP_SHOWN.load(Ordering::SeqCst));
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(!offer_latch_active());
        // offer_latch_off also drops the shown flag (webview dismissed it cleanly).
        latch_on_for(std::time::Duration::from_secs(60));
        offer_latch_off();
        assert!(!offer_latch_active());
        assert!(!OFFER_POPUP_SHOWN.load(Ordering::SeqCst));
    }
}
