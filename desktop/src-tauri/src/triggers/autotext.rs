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
use tauri::{Emitter, Manager};

use crate::db::Db;
use crate::domain::case_adapt;
use crate::domain::models::{Macro, Phrase, Segment, TriggerType};
use crate::domain::DomainResult;
use crate::triggers::Injector;

const MAX_BUFFER: usize = 64;

/// abbreviation -> phrase_ids. Literals are suffix-matched; regexes are pattern-matched
/// (a pattern triggers when some match ends at the buffer's end).
/// Also holds a spellcheck dictionary (Phase 5) built from phrase bodies.
pub struct AbbrevIndex {
    literals: HashMap<String, Vec<String>>,
    regexes: Vec<(regex::Regex, String)>,
    dict: std::collections::HashSet<String>,
}

impl AbbrevIndex {
    pub fn empty() -> Self {
        Self {
            literals: HashMap::new(),
            regexes: Vec::new(),
            dict: std::collections::HashSet::new(),
        }
    }

    pub fn from_phrases(phrases: &[Phrase]) -> Self {
        let mut literals: HashMap<String, Vec<String>> = HashMap::new();
        let mut regexes: Vec<(regex::Regex, String)> = Vec::new();
        let mut dict = std::collections::HashSet::new();
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
            // Phase 5: extract words from phrase body for the spellcheck dictionary.
            for seg in &p.body {
                if let Segment::Text { text } = seg {
                    for word in text.split(|c: char| !c.is_alphanumeric()) {
                        let w = word.to_lowercase();
                        if w.len() >= 3 {
                            dict.insert(w);
                        }
                    }
                }
            }
        }
        // Merge system English dictionary (loaded once, cached).
        for w in system_dictionary() {
            dict.insert(w.clone());
        }
        Self {
            literals,
            regexes,
            dict,
        }
    }

    pub fn len(&self) -> usize {
        self.literals.len() + self.regexes.len()
    }

    pub fn dict_len(&self) -> usize {
        self.dict.len()
    }

    /// Phase 5B: find the nearest dictionary word using BK-tree (fast for system dict)
    /// plus linear scan for phrase dict. Frequency weighting: words from the user's
    /// phrase bodies are preferred over system-dict-only words at the same distance.
    pub fn correct(&self, word: &str, ignore: &std::collections::HashSet<String>) -> Option<String> {
        let lower = word.to_lowercase();
        if self.dict.contains(&lower) {
            return None;
        }
        if ignore.contains(&lower) {
            return None;
        }
        if self.literals.keys().any(|abbr| abbr.to_lowercase() == lower) {
            return None;
        }
        let max_dist = if lower.chars().count() <= 4 { 1 } else { 2 };

        // Collect candidates: (distance, word, is_from_phrase_dict)
        let mut candidates: Vec<(usize, String, bool)> = Vec::new();

        // System dict: BK-tree search (fast — O(log n))
        for (d, w) in system_bk_tree().search(&lower, max_dist) {
            candidates.push((d, w, false));
        }

        // Phrase dict: linear scan (small — ~1.5K words)
        let sys = system_dictionary();
        for entry in &self.dict {
            if sys.contains(entry) {
                continue; // already searched via BK-tree
            }
            let d = damerau_levenshtein(&lower, entry);
            if d <= max_dist {
                candidates.push((d, entry.clone(), true));
            }
        }

        if candidates.is_empty() {
            return None;
        }

        // Sort: lowest distance first; at same distance, phrase-dict words first
        // (frequency weighting — the user has used these); then shorter words; then alphabetical.
        candidates.sort_by(|a, b| {
            a.0.cmp(&b.0) // distance
                .then(b.2.cmp(&a.2)) // phrase-dict first (true > false)
                .then(a.1.len().cmp(&b.1.len())) // shorter preferred
                .then(a.1.cmp(&b.1)) // alphabetical
        });

        Some(candidates[0].1.clone())
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

/// Phase 5: Damerau-Levenshtein distance — like Levenshtein but also detects adjacent
/// transpositions (teh→the = distance 1, not 2). No external dep.
#[allow(clippy::needless_range_loop)]
fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
            // Adjacent transposition: a[i-1]↔a[i-2] matches b[j-1]↔b[j-2]
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                dp[i][j] = dp[i][j].min(dp[i - 2][j - 2] + 1);
            }
        }
    }
    dp[m][n]
}

/// Lazily load the system English dictionary (once, then cached).
fn system_dictionary() -> &'static std::collections::HashSet<String> {
    use std::sync::OnceLock;
    static DICT: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    DICT.get_or_init(|| {
        let mut words = std::collections::HashSet::new();
        for path in [
            "/usr/share/dict/words",
            "/usr/share/dict/american-english",
            "/usr/share/dict/british-english",
        ] {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    let w = line.trim().to_lowercase();
                    if w.len() >= 3 && w.chars().all(|c| c.is_alphabetic()) {
                        words.insert(w);
                    }
                }
                break;
            }
        }
        words
    })
}

// --------------------------------------------------------------------------- //
// BK-tree for O(log n) fuzzy spellcheck lookup (Phase B).
// --------------------------------------------------------------------------- //
struct BkNode {
    word: String,
    children: std::collections::HashMap<usize, BkNode>,
}

struct BkTree {
    root: Option<BkNode>,
}

impl BkTree {
    fn new() -> Self {
        Self { root: None }
    }

    fn insert(&mut self, word: &str) {
        if word.is_empty() {
            return;
        }
        match &mut self.root {
            None => {
                self.root = Some(BkNode {
                    word: word.to_string(),
                    children: std::collections::HashMap::new(),
                });
            }
            Some(root) => bk_insert(root, word),
        }
    }

    /// Returns (distance, word) pairs within `max_dist` of `query`.
    fn search(&self, query: &str, max_dist: usize) -> Vec<(usize, String)> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            bk_search(root, query, max_dist, &mut results);
        }
        results
    }
}

fn bk_insert(node: &mut BkNode, word: &str) {
    let d = damerau_levenshtein(&node.word, word);
    if d == 0 {
        return; // duplicate
    }
    match node.children.get_mut(&d) {
        Some(child) => bk_insert(child, word),
        None => {
            node.children.insert(
                d,
                BkNode {
                    word: word.to_string(),
                    children: std::collections::HashMap::new(),
                },
            );
        }
    }
}

fn bk_search(node: &BkNode, query: &str, max_dist: usize, results: &mut Vec<(usize, String)>) {
    let d = damerau_levenshtein(&node.word, query);
    if d <= max_dist {
        results.push((d, node.word.clone()));
    }
    let lo = d.saturating_sub(max_dist).max(1);
    let hi = d + max_dist;
    for (dist, child) in &node.children {
        if *dist >= lo && *dist <= hi {
            bk_search(child, query, max_dist, results);
        }
    }
}

/// Lazily build a BK-tree from the system dictionary (once, cached forever).
fn system_bk_tree() -> &'static BkTree {
    use std::sync::OnceLock;
    static TREE: OnceLock<BkTree> = OnceLock::new();
    TREE.get_or_init(|| {
        let mut tree = BkTree::new();
        for word in system_dictionary() {
            tree.insert(word);
        }
        tree
    })
}

/// Phase 5: on a word boundary (space), check the last word against the dictionary
/// and auto-correct if a close match is found.
fn maybe_correct(
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    inj: &dyn Injector,
    inj_flag: &Arc<AtomicBool>,
    ignore: &Mutex<std::collections::HashSet<String>>,
) {
    let trimmed = buffer.trim_end();
    let word_start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
    let word = &trimmed[word_start..];
    if word.len() < 3 {
        return;
    }
    let ignore_set = ignore.lock().ok();
    let empty = std::collections::HashSet::new();
    let correction = idx.lock().ok().and_then(|i| {
        let ig: &std::collections::HashSet<String> = ignore_set.as_deref().unwrap_or(&empty);
        i.correct(word, ig)
    });
    if let Some(corrected) = correction {
        inj_flag.store(true, Ordering::Relaxed);
        let word_chars = word.chars().count();
        let _ = inj.backspace(word_chars + 1); // erase "word "
        let _ = inj.type_text(&format!("{corrected} "));
        inj_flag.store(false, Ordering::Relaxed);
        // Update the buffer to reflect the correction.
        let prefix = &buffer[..word_start.min(buffer.len())];
        *buffer = format!("{prefix}{corrected} ");
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

/// Does the phrase contain an AI macro (which needs a blocking network call)?
fn has_ai(p: &Phrase) -> bool {
    p.body.iter().any(|s| {
        matches!(
            s,
            Segment::Macro { macro_value }
                if matches!(macro_value, Macro::Ai { .. })
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
#[allow(clippy::too_many_arguments)]
pub fn start(
    db: Arc<Db>,
    injector: Arc<dyn Injector>,
    index: Arc<Mutex<AbbrevIndex>>,
    enabled: Arc<AtomicBool>,
    spellcheck: Arc<AtomicBool>,
    case_adapt: Arc<AtomicBool>,
    current: Arc<Mutex<Option<String>>>,
    ignore: Arc<Mutex<std::collections::HashSet<String>>>,
    app: tauri::AppHandle,
) -> AutotextHandle {
    let events_seen = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(false));
    let injecting = Arc::new(AtomicBool::new(false));

    let en = enabled;
    let sc = spellcheck;
    let ca = case_adapt;
    let seen = events_seen.clone();
    let run = running.clone();
    let idx = index.clone();
    let database = db.clone();
    let inj = injector.clone();
    let inj_flag = injecting.clone();
    let app_h = app.clone();
    let cur = current.clone();
    let ig = ignore.clone();

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
                        maybe_expand(&mut buffer, &idx, &database, &*inj, &inj_flag, &ca, &cur, &app_h);
                        if ch == ' ' && !buffer.is_empty() && sc.load(Ordering::Relaxed) {
                            maybe_correct(&mut buffer, &idx, &*inj, &inj_flag, &ig);
                        }
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

#[allow(clippy::too_many_arguments)]
fn maybe_expand(
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    db: &Db,
    inj: &dyn Injector,
    inj_flag: &Arc<AtomicBool>,
    case_adapt_flag: &AtomicBool,
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

    // AI macros make blocking network calls (1-3s). Erase the abbreviation now
    // and expand + inject in a background thread so the listener isn't blocked.
    if has_ai(&phrase) {
        inj_flag.store(true, Ordering::Relaxed);
        let _ = inj.backspace(abbr.chars().count());
        inj_flag.store(false, Ordering::Relaxed);
        buffer.clear();

        let state = app.state::<crate::AppState>();
        let db_arc = state.db.clone();
        let inj_arc = state.injector.clone();
        let phrase_owned = phrase.clone();
        let app_clone = app.clone();
        let case_adapt_on = case_adapt_flag.load(Ordering::Relaxed);
        let abbr_owned = abbr.clone();
        std::thread::spawn(move || {
            match db_arc.expand(&phrase_owned, &HashMap::new()) {
                Ok(output) => {
                    let output = if case_adapt_on {
                        crate::domain::case_adapt(&output, &abbr_owned)
                    } else {
                        output
                    };
                    let _ = crate::triggers::inject::deliver(
                        &*inj_arc,
                        phrase_owned.content_type,
                        &output,
                    );
                }
                Err(e) => {
                    eprintln!("[phrasexp] AI expansion error: {e}");
                    let _ = app_clone.emit("phrase-error", format!("{e}"));
                }
            }
        });
        return;
    }

    // Non-AI: expand synchronously (fast — pure computation, no network).
    let output = match db.expand(&phrase, &HashMap::new()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[phrasexp] expansion error: {e}");
            let _ = app.emit("phrase-error", format!("{e}"));
            inj_flag.store(true, Ordering::Relaxed);
            let _ = inj.backspace(abbr.chars().count());
            inj_flag.store(false, Ordering::Relaxed);
            buffer.clear();
            return;
        }
    };
    let output = if case_adapt_flag.load(Ordering::Relaxed) {
        case_adapt(&output, &abbr)
    } else {
        output
    };

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

    #[test]
    fn damerau_levenshtein_basics() {
        assert_eq!(damerau_levenshtein("hello", "hello"), 0);
        assert_eq!(damerau_levenshtein("teh", "the"), 1); // transposition = distance 1!
        assert_eq!(damerau_levenshtein("kitten", "sitting"), 3);
        assert_eq!(damerau_levenshtein("", "abc"), 3);
    }

    #[test]
    fn spellcheck_corrects_typos() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text {
                text: "the quick brown fox".into(),
            }],
        )]);
        // "teh" is a transposition typo → corrected to SOME valid word (the, tea, etc.)
        let correction = idx.correct("teh", &empty_ignore);
        assert!(correction.is_some(), "teh should be corrected");
        // "the" is already in dict → no correction
        assert!(idx.correct("the", &empty_ignore).is_none());
        // "xyzqq" has no close match → None
        assert!(idx.correct("xyzqq", &empty_ignore).is_none());
    }

    #[test]
    fn spellcheck_respects_ignore_list() {
        let mut ignore = std::collections::HashSet::new();
        ignore.insert("dcker".to_string());
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text { text: "docker".into() }],
        )]);
        // "dcker" would normally correct, but it's in the ignore list
        assert!(idx.correct("dcker", &ignore).is_none());
        // remove from ignore → corrects
        ignore.clear();
        assert!(idx.correct("dcker", &ignore).is_some());
    }

    #[test]
    fn spellcheck_protects_abbreviations() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("sig"),
            vec![Segment::Text { text: "signature".into() }],
        )]);
        // "sig" is a registered abbreviation → don't correct even if close to a dict word
        assert!(idx.correct("sig", &empty_ignore).is_none());
    }
}
