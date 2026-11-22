//! Silent autotext — type an abbreviation anywhere and it auto-expands.
//!
//! An rdev listener reconstructs typed characters from physical-key events (US layout),
//! buffers them, and on each character checks whether the buffer ends with a known
//! abbreviation. On match (and the phrase has no inputform macros) it erases the
//! abbreviation and types the expansion via the Injector.
//!
//! Platform constraints:
//! - Linux: the listener reads `/dev/input/event*` directly (`evdev_listen`) — the only
//!   source that sees native Wayland apps; rdev's built-in `listen()` is XRecord-only
//!   (XWayland apps) and is kept as the fallback. The user must be in the `input`
//!   group (or run privileged), or only the fallback is available.
//! - macOS: requires Accessibility permission.
//! - Windows: works by default.
//! - Wayland: supported via the evdev listener (needs the `input` group).
//!
//! Internally split into: `listen` (key listener + hotkey parsing), `expand` (expansion
//! engine), `spellcheck` (SymSpell/Soundex engine), `index` (abbreviation index), and
//! `offer` (spellcheck-offer popup flow). All public items are re-exported here so
//! `crate::triggers::autotext::…` paths keep working.

mod expand;
mod index;
mod listen;
mod offer;
mod spellcheck;

pub use index::*;
pub use listen::*;
pub use offer::*;
