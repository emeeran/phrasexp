//! Platform capability probes — macOS permission state, cheap and synchronous.
//!
//! Hand-written FFI to two plain C entry points (same idiom as the X11 FFI in
//! autotext.rs — no objc/runtime crates needed):
//!   * `AXIsProcessTrustedWithOptions` (ApplicationServices) — the Accessibility
//!     grant, which covers both global listening (CGEventTap) and injection (CGEvent
//!     posting). Without it macOS silently ignores the listener: the app looks alive
//!     (thread running, no errors) yet receives zero keystrokes.
//!   * `IsSecureEventInputEnabled` (Carbon/HIToolbox) — Secure Input, which macOS
//!     enables for password fields and which suppresses ALL global taps. To the user
//!     this reads as "PhraseXp randomly went dead"; surfacing it turns a support
//!     mystery into a visible state.

/// Permission state for the header + settings UI. `None` = not applicable on this OS.
pub fn permissions() -> Permissions {
    #[cfg(target_os = "macos")]
    {
        Permissions {
            accessibility_granted: Some(macos::accessibility_granted()),
            secure_input_active: Some(macos::secure_input_active()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Permissions {
            accessibility_granted: None,
            secure_input_active: None,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Permissions {
    pub accessibility_granted: Option<bool>,
    pub secure_input_active: Option<bool>,
}

#[cfg(target_os = "macos")]
mod macos {
    use std::os::raw::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        /// C `Boolean` (unsigned char): 0 = not trusted, 1 = trusted. A NULL options
        /// dict checks without firing the system prompt — we direct the user to
        /// System Settings from our own UI instead.
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
    }

    pub fn accessibility_granted() -> bool {
        // SAFETY: pure query; NULL options means no prompt side effect.
        unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) != 0 }
    }

    pub fn secure_input_active() -> bool {
        // SAFETY: pure query.
        unsafe { IsSecureEventInputEnabled() != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_options_are_null_off_macos() {
        // Off-macOS the probes are explicitly not applicable; the UI keys off `None`.
        #[cfg(not(target_os = "macos"))]
        {
            let p = permissions();
            assert_eq!(p.accessibility_granted, None);
            assert_eq!(p.secure_input_active, None);
        }
        // On macOS, compile-time proof the probes are wired (values are environment-
        // dependent and can't be asserted in CI runners without grants).
        #[cfg(target_os = "macos")]
        {
            let p = permissions();
            assert!(p.accessibility_granted.is_some());
            assert!(p.secure_input_active.is_some());
        }
    }
}
