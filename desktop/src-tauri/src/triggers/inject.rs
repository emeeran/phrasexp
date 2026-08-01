//! Text injection — the "type the expansion into the focused app" layer (ADR 0001).
//!
//! Session-aware: `EnigoInjector` for Windows / macOS / Linux-X11; `YdotoolInjector`
//! for Linux-Wayland (uinput-based, works where Wayland blocks synthetic input).
//! Silent autotext listening is a separate concern (rdev, M4).

use std::process::Command;
use std::sync::Arc;

use enigo::{Direction::Click, Enigo, Key, Keyboard, Settings};

use crate::domain::error::{DomainError, DomainResult};

/// Inject text / backspaces into the currently focused application.
pub trait Injector: Send + Sync {
    fn type_text(&self, text: &str) -> DomainResult<()>;
    fn backspace(&self, n: usize) -> DomainResult<()>;
}

/// Windows, macOS, Linux-X11 (enigo uses X11/XTest on Linux).
pub struct EnigoInjector;

impl Injector for EnigoInjector {
    fn type_text(&self, text: &str) -> DomainResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| DomainError::Validation(format!("enigo init: {e}")))?;
        // enigo 0.6: text()/key() return InputResult (ignored here); init is the fallible step.
        let _ = enigo.text(text);
        Ok(())
    }

    fn backspace(&self, n: usize) -> DomainResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| DomainError::Validation(format!("enigo init: {e}")))?;
        for _ in 0..n {
            let _ = enigo.key(Key::Backspace, Click);
        }
        Ok(())
    }
}

/// Linux-Wayland — shells out to `ydotool` (uinput virtual keyboard).
/// Requires `ydotool` installed + `ydotoole` daemon + `/dev/uinput` access.
pub struct YdotoolInjector;

impl Injector for YdotoolInjector {
    fn type_text(&self, text: &str) -> DomainResult<()> {
        let out = Command::new("ydotool")
            .args(["type", text])
            .output()
            .map_err(|e| DomainError::Validation(format!("ydotool not available: {e}")))?;
        if !out.status.success() {
            return Err(DomainError::Validation(format!(
                "ydotool type failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(())
    }

    fn backspace(&self, n: usize) -> DomainResult<()> {
        // Linux input code 14 = KEY_BACKSPACE; "14:1 14:0" = press + release.
        for _ in 0..n {
            let _ = Command::new("ydotool")
                .args(["key", "14:1", "14:0"])
                .status()
                .map_err(|e| DomainError::Validation(format!("ydotool key: {e}")))?;
        }
        Ok(())
    }
}

/// Pick the injector for the current OS/session (ADR 0001).
pub fn make_injector() -> Arc<dyn Injector> {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("XDG_SESSION_TYPE")
            .map(|s| s.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false);
        if wayland {
            return Arc::new(YdotoolInjector);
        }
    }
    Arc::new(EnigoInjector)
}
