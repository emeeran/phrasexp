//! Text injection — the "type the expansion into the focused app" layer (ADR 0001).
//!
//! Session-aware: `EnigoInjector` for Windows / macOS / Linux-X11; `YdotoolInjector`
//! for Linux-Wayland (uinput-based, works where Wayland blocks synthetic input).
//! Silent autotext listening is a separate concern (rdev, M4).

use std::process::Command;
use std::sync::Arc;

use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::ContentType;

/// Inject text / backspaces / rich paste into the currently focused application.
pub trait Injector: Send + Sync {
    fn type_text(&self, text: &str) -> DomainResult<()>;
    fn backspace(&self, n: usize) -> DomainResult<()>;
    /// Phase 3: put HTML on the clipboard and paste (Ctrl+V) — for Markdown phrases.
    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()>;
}

/// Windows, macOS, Linux-X11 (enigo uses X11/XTest on Linux).
pub struct EnigoInjector;

impl Injector for EnigoInjector {
    fn type_text(&self, text: &str) -> DomainResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| DomainError::Validation(format!("enigo init: {e}")))?;
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

    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()> {
        let mut clip = arboard::Clipboard::new()
            .map_err(|e| DomainError::Validation(format!("clipboard: {e}")))?;
        clip.set_html(html, Some(alt_text))
            .map_err(|e| DomainError::Validation(format!("set_html: {e}")))?;
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| DomainError::Validation(format!("enigo init: {e}")))?;
        // Ctrl+V
        let _ = enigo.key(Key::Control, Press);
        let _ = enigo.key(Key::Unicode('v'), Press);
        let _ = enigo.key(Key::Unicode('v'), Release);
        let _ = enigo.key(Key::Control, Release);
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

    fn paste_html(&self, html: &str, alt_text: &str) -> DomainResult<()> {
        let mut clip = arboard::Clipboard::new()
            .map_err(|e| DomainError::Validation(format!("clipboard: {e}")))?;
        clip.set_html(html, Some(alt_text))
            .map_err(|e| DomainError::Validation(format!("set_html: {e}")))?;
        // ydotool: 29=LeftCtrl, 47=v → press ctrl, press v, release v, release ctrl.
        let _ = Command::new("ydotool")
            .args(["key", "29:1", "47:1", "47:0", "29:0"])
            .status()
            .map_err(|e| DomainError::Validation(format!("ydotool key: {e}")))?;
        Ok(())
    }
}

/// Deliver an expansion according to its content type.
pub fn deliver(
    injector: &dyn Injector,
    content_type: ContentType,
    text: &str,
) -> DomainResult<()> {
    match content_type {
        ContentType::Markdown => injector.paste_html(&markdown_to_html(text), text),
        _ => injector.type_text(text),
    }
}

/// Render Markdown source to an HTML fragment (Phase 3).
fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
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
