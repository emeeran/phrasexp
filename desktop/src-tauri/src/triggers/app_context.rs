//! Focused-window detection (app-scoping).
//!
//! Best-effort: on Linux shells out to `xdotool`; Windows queries the foreground
//! window directly. macOS returns None for now (app-scoped phrases then fire
//! globally — graceful degradation; NSWorkspace frontmost-app is the upgrade path,
//! but window *titles* there need Screen Recording permission since 10.15).

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
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
        // SAFETY: plain Win32 queries; buffers are correctly sized and bounded.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if len == 0 {
                return None;
            }
            let s = String::from_utf16_lossy(&buf[..len as usize])
                .trim()
                .to_string();
            (!s.is_empty()).then_some(s)
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
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

/// True if the focused window's title contains any of the (lowercased) `needles`.
/// Case-insensitive. An undetectable window (None) or empty needle set → false, so
/// detection failures never silently disable features that use this.
pub fn title_matches_any(
    current: &Option<String>,
    needles: &std::collections::HashSet<String>,
) -> bool {
    if needles.is_empty() {
        return false;
    }
    match current {
        Some(title) => {
            let lower = title.to_lowercase();
            needles.iter().any(|n| lower.contains(n))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn title_matches_empty_or_unknown() {
        assert!(!title_matches_any(&Some("x".into()), &set(&[])));
        assert!(!title_matches_any(&None, &set(&["code"])));
    }

    #[test]
    fn title_matches_case_insensitive_substring() {
        let needles = set(&["code", "terminal"]);
        assert!(title_matches_any(
            &Some("main.rs — Visual Studio Code".into()),
            &needles
        ));
        assert!(title_matches_any(
            &Some("user@host: ~/ TERMINAL".into()),
            &needles
        ));
        assert!(!title_matches_any(
            &Some("Slack — general".into()),
            &needles
        ));
    }
}
