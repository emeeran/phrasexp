//! Silent autotext (M4) — type an abbreviation anywhere and it auto-expands.
//!
//! An rdev listener reconstructs typed characters from physical-key events (US layout),
//! buffers them, and on each character checks whether the buffer ends with a known
//! abbreviation. On match (and the phrase has no inputform macros) it erases the
//! abbreviation and types the expansion via the Injector.
//!
//! Platform constraints (ADR 0001):
//! - Linux: rdev reads global keys via evdev (`/dev/input/event*`) ⇒ the user must be in
//!   the `input` group (or run privileged). Without it, no events arrive.
//! - macOS: requires Accessibility permission.
//! - Windows: works by default.
//! - Wayland: unsupported (no global key capture).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rdev::{listen, Event, EventType, Key};

use crate::db::Db;
use crate::domain::case_adapt;
use crate::domain::models::{Macro, Phrase, Segment};
use crate::domain::DomainResult;
use crate::triggers::Injector;

const MAX_BUFFER: usize = 64;

/// abbreviation -> phrase_id, for non-deleted phrases that carry an abbreviation.
#[derive(Default)]
pub struct AbbrevIndex(HashMap<String, String>);

impl AbbrevIndex {
    pub fn empty() -> Self {
        Self(HashMap::new())
    }

    pub fn from_phrases(phrases: &[Phrase]) -> Self {
        let mut m = HashMap::new();
        for p in phrases {
            if let Some(a) = &p.abbreviation {
                if !a.is_empty() {
                    m.insert(a.clone(), p.id.clone());
                }
            }
        }
        Self(m)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Longest abbreviation the buffer ends with (so "sig" doesn't shadow "signature").
    /// Returns owned strings so callers need not hold the index lock.
    pub fn match_suffix(&self, buffer: &str) -> Option<(String, String)> {
        let mut best: Option<(&str, &str)> = None;
        for (abbr, id) in &self.0 {
            if buffer.ends_with(abbr) {
                match best {
                    Some((b, _)) if b.len() >= abbr.len() => {}
                    _ => best = Some((abbr.as_str(), id.as_str())),
                }
            }
        }
        best.map(|(a, i)| (a.to_string(), i.to_string()))
    }
}

fn is_shift(k: Key) -> bool {
    matches!(k, Key::ShiftLeft | Key::ShiftRight)
}
fn is_modifier(k: Key) -> bool {
    matches!(
        k,
        Key::ShiftLeft | Key::ShiftRight | Key::ControlLeft | Key::ControlRight
            | Key::Alt | Key::AltGr | Key::MetaLeft | Key::MetaRight
    )
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

fn has_inputform(p: &Phrase) -> bool {
    p.body.iter().any(|s| {
        matches!(
            s,
            Segment::Macro { macro_value }
                if matches!(macro_value, Macro::InputForm { .. })
        )
    })
}

/// US-layout mapping of a physical key (+ shift state) to the typed character, if printable.
/// Unknown/unmapped keys return None (the caller treats that as "reset the buffer").
pub fn key_to_char(key: Key, shifted: bool) -> Option<char> {
    use Key::*;
    let c = match key {
        KeyA => 'a', KeyB => 'b', KeyC => 'c', KeyD => 'd', KeyE => 'e', KeyF => 'f',
        KeyG => 'g', KeyH => 'h', KeyI => 'i', KeyJ => 'j', KeyK => 'k', KeyL => 'l',
        KeyM => 'm', KeyN => 'n', KeyO => 'o', KeyP => 'p', KeyQ => 'q', KeyR => 'r',
        KeyS => 's', KeyT => 't', KeyU => 'u', KeyV => 'v', KeyW => 'w', KeyX => 'x',
        KeyY => 'y', KeyZ => 'z',
        Num0 => '0', Num1 => '1', Num2 => '2', Num3 => '3', Num4 => '4',
        Num5 => '5', Num6 => '6', Num7 => '7', Num8 => '8', Num9 => '9',
        Space => ' ',
        Comma => ',', Dot => '.', Slash => '/', SemiColon => ';',
        Minus => '-', Equal => '=',
        _ => return None,
    };
    Some(if shifted {
        match c {
            'a'..='z' => c.to_ascii_uppercase(),
            '1' => '!', '2' => '@', '3' => '#', '4' => '$', '5' => '%',
            '6' => '^', '7' => '&', '8' => '*', '9' => '(', '0' => ')',
            '-' => '_', '=' => '+', ',' => '<', '.' => '>', '/' => '?', ';' => ':',
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
pub fn start(
    db: Arc<Db>,
    injector: Arc<dyn Injector>,
    index: Arc<Mutex<AbbrevIndex>>,
    enabled: Arc<AtomicBool>,
) -> AutotextHandle {
    let events_seen = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(false));
    let injecting = Arc::new(AtomicBool::new(false));

    let en = enabled;
    let seen = events_seen.clone();
    let run = running.clone();
    let idx = index.clone();
    let database = db.clone();
    let inj = injector.clone();
    let inj_flag = injecting.clone();

    std::thread::spawn(move || {
        run.store(true, Ordering::Relaxed);
        let mut buffer = String::new();
        let mut shifted = false;
        let mut modifier_active = false;

        let callback = move |event: Event| {
            seen.fetch_add(1, Ordering::Relaxed);
            // Skip our own injected keystrokes (enigo/XTest re-enters the listener).
            if inj_flag.load(Ordering::Relaxed) {
                return;
            }
            if !en.load(Ordering::Relaxed) {
                return;
            }
            match event.event_type {
                EventType::KeyPress(key) => {
                    if is_shift(key) {
                        shifted = true;
                        return;
                    }
                    if is_modifier(key) {
                        modifier_active = true;
                        return;
                    }
                    if modifier_active {
                        return; // part of a keyboard shortcut, not typing
                    }
                    if key == Key::Backspace {
                        buffer.pop();
                        return;
                    }
                    if clears_buffer(key) {
                        buffer.clear();
                        return;
                    }
                    if let Some(ch) = key_to_char(key, shifted) {
                        buffer.push(ch);
                        if buffer.len() > MAX_BUFFER {
                            let drop_n = buffer.len() - MAX_BUFFER;
                            buffer.drain(0..drop_n);
                        }
                        maybe_expand(&mut buffer, &idx, &database, &inj, &inj_flag);
                    } else {
                        // Unmapped key: the buffer is no longer a reliable suffix.
                        buffer.clear();
                    }
                }
                EventType::KeyRelease(key) => {
                    if is_shift(key) {
                        shifted = false;
                    }
                    if is_modifier(key) {
                        modifier_active = false;
                    }
                }
                _ => {}
            }
        };
        if let Err(e) = listen(callback) {
            eprintln!("rdev listen error: {e:?}");
        }
        run.store(false, Ordering::Relaxed);
    });

    AutotextHandle {
        events_seen,
        running,
    }
}

fn maybe_expand(
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    db: &Db,
    inj: &Arc<dyn Injector>,
    inj_flag: &Arc<AtomicBool>,
) {
    let matched = idx
        .lock()
        .map(|i| i.match_suffix(buffer))
        .ok()
        .flatten();
    let Some((abbr, id)) = matched else { return };

    let Ok(Some(phrase)) = db.get_phrase(&id) else {
        return;
    };
    if phrase.deleted_at.is_some() || has_inputform(&phrase) {
        return; // inputform phrases need the popup; skip silent expansion.
    }
    let Ok(output) = db.expand(&phrase, &HashMap::new()) else {
        return;
    };
    let output = case_adapt(&output, &abbr);

    // Erase the typed abbreviation, then type the expansion. Guard against re-entry.
    inj_flag.store(true, Ordering::Relaxed);
    let n = abbr.chars().count();
    let _ = inj.backspace(n);
    let _ = inj.type_text(&output);
    inj_flag.store(false, Ordering::Relaxed);
    buffer.clear();
}

/// Rebuild the abbreviation index from the DB (call after phrase mutations).
pub fn rebuild(db: &Db, index: &Mutex<AbbrevIndex>) -> DomainResult<()> {
    let phrases = db.list_phrases(None, None, false)?;
    let fresh = AbbrevIndex::from_phrases(&phrases);
    if let Ok(mut guard) = index.lock() {
        *guard = fresh;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::ContentType;
    use rdev::Key;

    fn phrase(id: &str, abbr: Option<&str>, body: Vec<Segment>) -> Phrase {
        Phrase {
            id: id.into(),
            folder_id: "f".into(),
            name: id.into(),
            abbreviation: abbr.map(String::from),
            content_type: ContentType::Plain,
            body,
            triggers: vec![],
            sensitive: false,
            created_at: "t".into(),
            updated_at: "t".into(),
            deleted_at: None,
        }
    }

    #[test]
    fn index_built_from_abbreviations() {
        let phrases = vec![
            phrase("p1", Some("br,"), vec![Segment::Text { text: "Best regards,".into() }]),
            phrase("p2", None, vec![Segment::Text { text: "x".into() }]),
            phrase("p3", Some("sig"), vec![Segment::Text { text: "S".into() }]),
        ];
        let idx = AbbrevIndex::from_phrases(&phrases);
        assert_eq!(idx.len(), 2);
    }

    #[test]
    fn suffix_match_basic() {
        let idx = AbbrevIndex::from_phrases(&[
            phrase("p1", Some("br,"), vec![Segment::Text { text: "x".into() }]),
            phrase("p2", Some("sig"), vec![Segment::Text { text: "x".into() }]),
        ]);
        assert_eq!(idx.match_suffix("hello br,").unwrap().0, "br,");
        assert_eq!(idx.match_suffix("xxsig").unwrap().0, "sig");
        assert!(idx.match_suffix("nomatch").is_none());
    }

    #[test]
    fn longest_suffix_wins() {
        let idx = AbbrevIndex::from_phrases(&[
            phrase("p1", Some("sig"), vec![Segment::Text { text: "x".into() }]),
            phrase("p2", Some("signature"), vec![Segment::Text { text: "x".into() }]),
        ]);
        assert_eq!(idx.match_suffix("mysignature").unwrap().0, "signature");
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
    fn inputform_detected() {
        let with_input = phrase(
            "p",
            Some("hi"),
            vec![Segment::Macro {
                macro_value: Macro::InputForm { field: "n".into() },
            }],
        );
        let plain = phrase(
            "p",
            Some("hi"),
            vec![Segment::Text { text: "Hi".into() }],
        );
        assert!(has_inputform(&with_input));
        assert!(!has_inputform(&plain));
    }
}
