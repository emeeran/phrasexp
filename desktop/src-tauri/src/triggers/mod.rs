//! Trigger subsystem: hotkeys, autotext, clipboard capture, and input injection (ADR 0001).

pub mod ai;
pub mod app_context;
pub mod autotext;
pub mod clipboard;
pub mod inject;

pub use autotext::{rebuild as rebuild_index, AbbrevIndex};
pub use inject::{make_injector, Injector};
