//! Trigger subsystem: hotkeys, autotext, clipboard capture, and input injection.

pub mod ai;
pub mod app_context;
pub mod autotext;
pub mod clipboard;
#[cfg(target_os = "linux")]
pub mod evdev_listen;
pub mod inject;

pub use autotext::{rebuild as rebuild_index, AbbrevIndex};
pub use inject::{make_injector, Injector};
