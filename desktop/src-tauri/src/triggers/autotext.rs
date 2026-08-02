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
use crate::domain::models::{Macro, Phrase, Segment, TriggerType};
use crate::domain::DomainResult;
use crate::triggers::Injector;

const MAX_BUFFER: usize = 64;

/// abbreviation -> phrase_ids. Literals are suffix-matched; regexes are pattern-matched
/// (a pattern triggers when some match ends at the buffer's end).
pub struct AbbrevIndex {
    literals: HashMap<String, Vec<String>>,
    regexes: Vec<(regex::Regex, String)>,
}

impl AbbrevIndex {
    pub fn empty() -> Self {
        Self {
            literals: HashMap::new(),
            regexes: Vec::new(),
        }
    }

    pub fn from_phrases(phrases: &[Phrase]) -> Self {
        let mut literals: HashMap<String, Vec<String>> = HashMap::new();
        let mut regexes: Vec<(regex::Regex, String)> = Vec::new();
        for p in phrases {
            let is_regex = p
                .triggers
                .iter()
                .any(|t| t.kind == TriggerType::Autotext && t.regex);
            let Some(abbr) = &p.abbreviation else { continue };
            if abbr.is_empty() {
                continue;
            }
            if is_regex {
                if let Ok(re) = regex::Regex::new(abbr) {
                    regexes.push((re, p.id.clone()));
                }
            } else {
                literals.entry(abbr.clone()).or_default().push(p.id.clone());
            }
        }
        Self { literals, regexes }
    }

    pub fn len(&self) -> usize {
        self.literals.len() + self.regexes.len()
    }

    /// Longest literal suffix first, then regexes. Returns (text to erase, candidate ids).
    pub fn match_suffix(&self, buffer: &str) -> Option<(String, Vec<String>)> {
        let mut best: Option<(&str, &Vec<String>)> = None;
        for (abbr, ids) in &self.literals {
            if buffer.ends_with(abbr) {
                match best {
                    Some((b, _)) if b.len() >= abbr.len() => {}
                    _ => best = Some((abbr.as_str(), ids)),
                }
            }
        }
        if let Some((a, ids)) = best {
            return Some((a.to_string(), ids.clone()));
        }

        // Regexes: trigger when a match ends at the buffer's end.
        let mut ids = Vec::new();
        let mut erase: Option<String> = None;
        for (re, id) in &self.regexes {
            if let Some(text) = regex_suffix_match(re, buffer) {
                ids.push(id.clone());
                if erase.is_none() {
                    erase = Some(text);
                }
            }
        }
        if ids.is_empty() {
            None
        } else {
            Some((erase.unwrap(), ids))
        }
    }
}

/// The text of the last regex match that ends at the buffer's end, if any.
fn regex_suffix_match(re: &regex::Regex, buffer: &str) -> Option<String> {
    let mut found = None;
    for m in re.find_iter(buffer) {
        if m.end() == buffer.len() {
            found = Some(m.as_str().to_string());
        }
    }
    found
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
    current: Arc<Mutex<Option<String>>>,
    app: tauri::AppHandle,
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
    let app_h = app.clone();
    let cur = current.clone();

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
                        maybe_expand(&mut buffer, &idx, &database, &*inj, &inj_flag, &cur, &app_h);
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
    inj: &dyn Injector,
    inj_flag: &Arc<AtomicBool>,
    current: &Mutex<Option<String>>,
    app: &tauri::AppHandle,
) {
    let matched = idx
        .lock()
        .map(|i| i.match_suffix(buffer))
        .ok()
        .flatten();
    let Some((abbr, ids)) = matched else {
        return;
    };

    if ids.len() > 1 {
        // Multi-match: erase the typed abbreviation and surface a chooser.
        inj_flag.store(true, Ordering::Relaxed);
        let _ = inj.backspace(abbr.chars().count());
        inj_flag.store(false, Ordering::Relaxed);
        buffer.clear();
        show_chooser(app, ids);
        return;
    }

    let id = &ids[0];
    let Ok(Some(phrase)) = db.get_phrase(id) else {
        return;
    };
    if phrase.deleted_at.is_some() || has_inputform(&phrase) {
        return; // inputform phrases need the popup; skip silent expansion.
    }
    // Phase 2: app scoping — only fire for this phrase if its app_scope matches the focused window.
    if let Some(scope) = &phrase.app_scope {
        let cur = current.lock().map(|g| g.clone()).ok().flatten();
        if !crate::triggers::app_context::scope_active(scope, &cur) {
            return; // wrong application focused
        }
    }
    let Ok(output) = db.expand(&phrase, &HashMap::new()) else {
        return;
    };
    let output = case_adapt(&output, &abbr);

    // Erase the typed abbreviation, then type the expansion. Guard against re-entry.
    inj_flag.store(true, Ordering::Relaxed);
    let n = abbr.chars().count();
    let _ = inj.backspace(n);
    let _ = crate::triggers::inject::deliver(inj, phrase.content_type, &output);
    inj_flag.store(false, Ordering::Relaxed);
    buffer.clear();
}

/// Surface the multi-match chooser: show the popup near the cursor + emit candidate ids.
fn show_chooser(app: &tauri::AppHandle, ids: Vec<String>) {
    use device_query::{DeviceQuery, DeviceState};
    use tauri::{Emitter, Manager, PhysicalPosition};
    if let Some(win) = app.get_webview_window("popup") {
        let (mx, my) = DeviceState::new().get_mouse().coords;
        let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(
            (mx + 8).max(0),
            (my + 8).max(0),
        )));
        let _ = win.show();
        let _ = win.set_focus();
    }
    let _ = app.emit("autotext-choose", ids);
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
            app_scope: None,
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
                macro_value: Macro::InputForm {
                    field: "n".into(),
                    kind: Default::default(),
                    options: vec![],
                },
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

    #[test]
    fn regex_abbreviation_matches_suffix() {
        use crate::domain::models::{TriggerBinding, TriggerType};
        let mut p = phrase(
            "pr",
            Some(":[a-z]+"),
            vec![Segment::Text { text: "🙂".into() }],
        );
        p.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some(":[a-z]+".into()),
            key: None,
            regex: true,
        }];
        let idx = AbbrevIndex::from_phrases(&[p]);
        let (erase, ids) = idx.match_suffix("hi :wave").unwrap();
        assert_eq!(erase, ":wave");
        assert_eq!(ids.len(), 1);
        // doesn't match when the pattern isn't a suffix
        assert!(idx.match_suffix(":wave middle").is_none());
    }
}
