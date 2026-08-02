//! Focused-window detection (Phase 2 app-scoping).
//!
//! Best-effort: on Linux shells out to `xdotool`; other platforms return None
//! (app-scoped phrases then fire globally — graceful degradation).

/// The title of the currently focused window, if it can be determined.
pub fn current_window_title() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("xdotool")
            .args(["getwindowfocus", "getwindowname"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// True if `phrase_scope` (a substring) is active under the current focused window.
/// If the scope is empty or the window can't be detected, treat as active (fire).
pub fn scope_active(phrase_scope: &str, current: &Option<String>) -> bool {
    if phrase_scope.is_empty() {
        return true;
    }
    match current {
        Some(title) => title.contains(phrase_scope),
        None => true, // undetectable -> don't block
    }
}
