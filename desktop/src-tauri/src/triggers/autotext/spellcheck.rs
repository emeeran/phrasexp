//! Spellcheck engine for autotext: SymSpell fuzzy lookup, Soundex fallback,
//! keyboard-weighted distance ranking, token splitting, and the silent-correction
//! pass (`maybe_correct`) driven by the key listener.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::db::Db;

use super::index::AbbrevIndex;
use super::listen::inject_with_suppression;

/// SymSpell symmetric-delete max edit distance the indexes are built for. Lookups may
/// request any distance ≤ this. 2 covers the "normal"/"aggressive" word distances;
/// "conservative" requests 1. Matches beyond this fall back to Soundex.
pub(super) const SYMSPELL_MAX_EDIT: usize = 2;

/// QWERTY rows used for keyboard-adjacency weighting of substitutions. A substitution of
/// a key adjacent to the intended one ("arm"→"atm": r↔t are neighbours) is a more likely
/// typo than one across the keyboard, so it ranks ahead when edit distance ties.
const QWERTY_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// Straight-line keyboard distance between two letters, measured on the physical QWERTY
/// grid (rows are offset by 0.5 keys, as on a real keyboard). Non-letters → 4 (far).
/// Used only as a ranking tiebreak on the handful of verified candidates, never inside
/// SymSpell generation, so the cost is a few comparisons per candidate.
fn key_distance(a: char, b: char) -> f32 {
    if a == b {
        return 0.0;
    }
    let pos = |c: char| {
        for (r, row) in QWERTY_ROWS.iter().enumerate() {
            if let Some(i) = row.find(c) {
                return (r as f32, i as f32 + r as f32 * 0.5); // staggered rows
            }
        }
        (4.0, 4.0) // off-keyboard (non-ASCII etc.): treat as far
    };
    let (ar, ac) = pos(a);
    let (br, bc) = pos(b);
    ((ar - br).powi(2) + (ac - bc).powi(2)).sqrt()
}

/// Damerau-Levenshtein where substitution cost scales with keyboard proximity: swapping
/// neighbouring keys costs ~0.6, opposite corners ~1.4. Same transposition handling as
/// `damerau_levenshtein`; returns a float cost. Ranking-only (see `key_distance`).
#[allow(clippy::needless_range_loop)]
pub(super) fn weighted_damerau_levenshtein(a: &str, b: &str) -> f32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n as f32;
    }
    if n == 0 {
        return m as f32;
    }
    let sub_cost = |x: char, y: char| -> f32 {
        if x == y {
            0.0
        } else {
            // adjacent keys ≈ 0.6, diagonal ≈ 0.9, far ≈ up to ~1.4 — kept below the
            // 2.0 of a deletion+insertion pair so integer distance stays the primary key
            (0.6 + key_distance(x, y) * 0.2).min(1.4)
        }
    };
    // Three rolling rows suffice: the recurrence reads row i-1 and (for
    // transpositions) i-2 — no full (m+1)×(n+1) matrix per candidate.
    let mut prev2 = vec![0f32; n + 1];
    let mut prev1: Vec<f32> = (0..=n).map(|j| j as f32).collect();
    let mut cur = vec![0f32; n + 1];
    for i in 1..=m {
        cur[0] = i as f32;
        for j in 1..=n {
            cur[j] = (prev1[j] + 1.0)
                .min(cur[j - 1] + 1.0)
                .min(prev1[j - 1] + sub_cost(a[i - 1], b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(prev2[j - 2] + 1.0);
            }
        }
        std::mem::swap(&mut prev2, &mut prev1);
        std::mem::swap(&mut prev1, &mut cur);
    }
    prev1[n]
}

/// Tier 1: Apply the original word's case pattern to the correction.
/// "Teh" → "The" (Title), "SIG" → "SIGN" (ALL CAPS), "helo" → "help" (lowercase).
pub(super) fn apply_case(original: &str, correction: &str) -> String {
    let mut orig_chars = original.chars();
    let first = orig_chars.next();
    let rest_upper = orig_chars.all(|c| c.is_uppercase());
    match first {
        Some(f) if f.is_uppercase() && rest_upper => correction.to_uppercase(),
        Some(f) if f.is_uppercase() => {
            // Title case: first letter uppercase, rest lowercase
            let mut c = correction.chars();
            match c.next() {
                Some(first_c) => first_c.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        }
        _ => correction.to_string(),
    }
}

/// Soundex phonetic code — catches "sounds like" typos that
/// Damerau-Levenshtein misses (e.g. "definately" → "definitely").
pub(super) fn soundex(s: &str) -> String {
    let chars: Vec<char> = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(4);
    result.push(chars[0].to_ascii_uppercase());
    let mut last_digit = soundex_digit(chars[0]);
    for &c in &chars[1..] {
        let d = soundex_digit(c);
        if d != '0' && d != last_digit {
            result.push(d);
            if result.len() >= 4 {
                break;
            }
        }
        if d != '0' {
            last_digit = d;
        } else {
            last_digit = '0';
        }
    }
    while result.len() < 4 {
        result.push('0');
    }
    result
}

fn soundex_digit(c: char) -> char {
    match c.to_ascii_lowercase() {
        'b' | 'f' | 'p' | 'v' => '1',
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => '2',
        'd' | 't' => '3',
        'l' => '4',
        'm' | 'n' => '5',
        'r' => '6',
        _ => '0', // a, e, i, o, u, y, h, w
    }
}

/// Damerau-Levenshtein distance — like Levenshtein but also detects adjacent
/// transpositions (teh→the = distance 1, not 2). No external dep.
#[allow(clippy::needless_range_loop)]
pub(super) fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    // Three rolling rows (see `weighted_damerau_levenshtein`).
    let mut prev2 = vec![0usize; n + 1];
    let mut prev1: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev1[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev1[j - 1] + cost);
            // Adjacent transposition: a[i-1]↔a[i-2] matches b[j-1]↔b[j-2]
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(prev2[j - 2] + 1);
            }
        }
        std::mem::swap(&mut prev2, &mut prev1);
        std::mem::swap(&mut prev1, &mut cur);
    }
    prev1[n]
}

/// Bundled English dictionary — tier 1 (25K words, ASCII letters, internal `'`/`-`, ≥3
/// chars), FREQUENCY-SORTED most-frequent-first (Norvig/Google unigram counts; see
/// `tools/gen_dict.py`). Order is load-bearing: the SymSpell insertion index doubles as
/// the word's frequency rank, so ranking needs no separate data structure. Covers the
/// symmetric-delete index + Soundex buckets. Embedded so spellcheck works identically on
/// Windows/macOS/Linux with no runtime file dependency. To add a language, swap/extend
/// this asset — the loader is format-agnostic.
static EMBEDDED_DICT: &str = include_str!("../dict_english.txt");

/// Bundled English dictionary — tier 2 (40K rarer words): membership-only, so
/// rare-but-valid words (proper nouns' lowercase forms, technical terms past the
/// typing-frequency cutoff) are known and never flagged as typos, without paying for a
/// deletes index over them. Plain word set.
static EMBEDDED_DICT_EXTRA: &str = include_str!("../dict_english_extra.txt");

/// The static spellcheck index over the bundled dictionary: a SymSpell symmetric-delete
/// map for fast fuzzy lookup, plus a Soundex bucket map for the phonetic fallback. Built
/// once, lazily, on first use.
pub(super) struct BaselineIndex {
    pub(super) syms: SymSpell,
    /// tier-2 membership vocabulary (no deletes index — memory cap).
    pub(super) extra: std::collections::HashSet<String>,
    /// soundex code → indices into `syms.words` (no duplicated word strings).
    pub(super) soundex: HashMap<String, Vec<u32>>,
}

pub(super) fn baseline_index() -> &'static BaselineIndex {
    use std::sync::OnceLock;
    static IDX: OnceLock<BaselineIndex> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut syms = SymSpell::new(SYMSPELL_MAX_EDIT);
        let mut extra: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut soundex_map: HashMap<String, Vec<u32>> = HashMap::new();
        for word in EMBEDDED_DICT.lines() {
            let w = word.trim();
            if w.is_empty() {
                continue;
            }
            let idx = syms.add(w);
            soundex_map.entry(soundex(w)).or_default().push(idx);
        }
        for word in EMBEDDED_DICT_EXTRA.lines() {
            let w = word.trim();
            if !w.is_empty() {
                extra.insert(w.to_string());
            }
        }
        // Bulk load done: compact the delete index into the flat layout.
        syms.freeze();
        BaselineIndex {
            syms,
            extra,
            soundex: soundex_map,
        }
    })
}

/// Curated high-confidence English bigrams (mostly function-word pairs) used as a
/// lightweight context signal when ranking candidates: a candidate attested after the
/// previous word beats raw frequency alone (after "a", "lot" outranks "lit"). The
/// phrase-body bigrams cover user-specific context; this table covers generic grammar.
pub(super) fn common_bigram(prev: &str, next: &str) -> bool {
    matches!(
        (prev, next),
        ("a", "lot")
            | ("a", "bit")
            | ("a", "few")
            | ("a", "little")
            | ("the", "way")
            | ("the", "same")
            | ("the", "other")
            | ("the", "time")
            | ("the", "end")
            | ("this", "time")
            | ("that", "time")
            | ("in", "the")
            | ("on", "the")
            | ("of", "the")
            | ("to", "the")
            | ("at", "the")
            | ("for", "the")
            | ("with", "the")
            | ("from", "the")
            | ("about", "the")
            | ("is", "not")
            | ("are", "not")
            | ("was", "not")
            | ("were", "not")
            | ("will", "be")
            | ("would", "be")
            | ("can", "be")
            | ("could", "be")
            | ("should", "be")
            | ("have", "been")
            | ("has", "been")
            | ("had", "been")
            | ("will", "have")
            | ("would", "have")
            | ("could", "have")
            | ("should", "have")
            | ("might", "have")
            | ("must", "have")
            | ("i", "am")
            | ("i", "have")
            | ("i", "will")
            | ("i", "would")
            | ("you", "are")
            | ("you", "have")
            | ("you", "will")
            | ("we", "are")
            | ("we", "have")
            | ("we", "will")
            | ("they", "are")
            | ("they", "have")
    )
}

// --------------------------------------------------------------------------- //
// SymSpell — symmetric-delete fuzzy lookup. For each dictionary word we store every
// string reachable by deleting ≤k characters; a query does the same and intersects. This
// turns fuzzy search into hash lookups (no per-word distance scan across the whole
// dictionary), and the user vocabulary can be added incrementally without a full rebuild.
// --------------------------------------------------------------------------- //

/// FNV-1a 64-bit — the delete-map key. Delete strings are only ever
/// exact-looked-up (never recovered or enumerated), so the map stores the 8-byte
/// hash instead of the string — ~100 MB → tens of MB at dictionary scale. A hash
/// collision merely merges two key lists, and every candidate is verified with
/// true Damerau-Levenshtein below, so a collision cannot surface a wrong word.
fn delete_key_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(super) struct SymSpell {
    max_edit: usize,
    pub(super) words: Vec<String>,
    /// word → frequency rank (insertion order index). Replaces the old HashSet `set` —
    /// membership and rank in one map, same order of memory.
    rank: HashMap<String, u32>,
    /// Build state: flat (delete-hash, word index) pairs accumulated by `add`.
    /// Flat ON PURPOSE — the old per-key `HashMap<u64, Vec<u32>>` peaked at
    /// ~100 MB RSS for the 25K-word dictionary because glibc never returns the
    /// freed intermediate to the OS. Pairs sort in place and emit the frozen
    /// layout; no fat intermediate map ever exists.
    pending: Vec<(u64, u32)>,
    /// Frozen lookup structure (see `FrozenDeletes`), built by `freeze()`.
    /// `add` after a freeze re-accumulates into `pending` until the next freeze.
    frozen: Option<FrozenDeletes>,
}

/// Flattened delete index. `table` is sorted by hash for binary search; each row
/// points into the concatenated `entries` vector. Hash collisions (and merges of
/// a re-freeze) yield a covering span — safe, because candidates are deduped and
/// true-distance-verified in `lookup`, so a wider span can only add verified hits.
struct FrozenDeletes {
    entries: Vec<u32>,
    table: Vec<(u64, u32, u32)>, // (hash, start, len), sorted by hash
}

impl FrozenDeletes {
    fn idxs(&self, hash: u64) -> &[u32] {
        match self.table.binary_search_by_key(&hash, |r| r.0) {
            Ok(i) => {
                let (s, l) = (self.table[i].1, self.table[i].2);
                &self.entries[s as usize..(s + l) as usize]
            }
            Err(_) => &[],
        }
    }
}

impl SymSpell {
    pub(super) fn new(max_edit: usize) -> Self {
        Self {
            max_edit,
            words: Vec::new(),
            rank: HashMap::new(),
            pending: Vec::new(),
            frozen: None,
        }
    }

    pub(super) fn contains(&self, word: &str) -> bool {
        self.rank.contains_key(word)
    }

    /// Distinct delete keys in the frozen table (diagnostics; test-only — the
    /// shipped app never inspects the index shape).
    #[cfg(test)]
    pub(super) fn delete_key_count(&self) -> usize {
        self.frozen.as_ref().map(|f| f.table.len()).unwrap_or(0)
    }

    /// Frequency rank of an indexed word (lower = more frequent; insertion order for the
    /// frequency-sorted bundled dict). None for non-indexed words.
    pub(super) fn rank_of(&self, word: &str) -> Option<u32> {
        self.rank.get(word).copied()
    }

    /// Index a word (idempotent). Records all symmetric deletes up to `max_edit`.
    /// Callers loading a frequency-sorted list should preserve its order — insertion
    /// index doubles as the rank returned by `rank_of`. Returns the word's index
    /// (existing words return their established index), so Soundex buckets can
    /// reference words by position instead of duplicating the strings.
    pub(super) fn add(&mut self, word: &str) -> u32 {
        let w = word.to_lowercase();
        if let Some(&idx) = self.rank.get(&w) {
            return idx;
        }
        let idx = self.words.len() as u32;
        self.words.push(w.clone());
        self.rank.insert(w.clone(), idx);
        self.pending.extend(
            deletes_of(&w, self.max_edit)
                .into_iter()
                .map(|d| (delete_key_hash(&d), idx)),
        );
        idx
    }

    /// Compact `pending` into the frozen flat layout. Call once after bulk
    /// loading (dictionary load, index rebuild); later `add`s accumulate until
    /// the next freeze — nothing in the app adds after freezing.
    pub(super) fn freeze(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let mut frozen = self.frozen.take().unwrap_or(FrozenDeletes {
            entries: Vec::new(),
            table: Vec::new(),
        });
        // Sort the new pairs by hash, then walk the runs, emitting one entries
        // span per distinct hash. In-place sort of a flat vector — no per-key
        // allocation, so build peak ≈ final size.
        self.pending.sort_unstable_by_key(|&(h, _)| h);
        let pairs = std::mem::take(&mut self.pending);
        let mut run_start: Option<u32> = None;
        let mut run_hash = 0u64;
        for (h, i) in pairs {
            if run_start.is_none() || run_hash != h {
                if let Some(start) = run_start {
                    let len = frozen.entries.len() as u32 - start;
                    frozen.table.push((run_hash, start, len));
                }
                run_start = Some(frozen.entries.len() as u32);
                run_hash = h;
            }
            frozen.entries.push(i);
        }
        if let Some(start) = run_start {
            let len = frozen.entries.len() as u32 - start;
            frozen.table.push((run_hash, start, len));
        }
        // Fold the new rows into any previous freeze (the app never adds after
        // freezing; this exists for API completeness) by merging duplicate-hash
        // rows into covering spans (see FrozenDeletes docs).
        if !frozen.table.is_empty() {
            frozen.table.sort_unstable_by_key(|r| r.0);
            let mut merged: Vec<(u64, u32, u32)> = Vec::with_capacity(frozen.table.len());
            for &(h, s, l) in &frozen.table {
                match merged.last_mut() {
                    Some(last) if last.0 == h => {
                        let end = (last.1 + last.2).max(s + l);
                        last.1 = last.1.min(s);
                        last.2 = end - last.1;
                    }
                    _ => merged.push((h, s, l)),
                }
            }
            frozen.table = merged;
        }
        frozen.entries.shrink_to_fit();
        frozen.table.shrink_to_fit();
        self.frozen = Some(frozen);
    }

    /// All (distance, word) pairs within `max_dist` of `query`. The query's own deletes
    /// (plus the query itself) are looked up in the delete index; each hit is verified
    /// with true Damerau-Levenshtein distance. The exact query never self-matches
    /// (distance 0). An unfrozen index (test sizes) scans `pending` linearly.
    pub(super) fn lookup(&self, query: &str, max_dist: usize) -> Vec<(usize, &str)> {
        let max_dist = max_dist.min(self.max_edit);
        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut out: Vec<(usize, &str)> = Vec::new();
        let mut cands: Vec<String> = vec![query.to_string()];
        cands.extend(deletes_of(query, max_dist));
        for c in &cands {
            let hash = delete_key_hash(c);
            let hit: Vec<u32> = match &self.frozen {
                Some(f) => f.idxs(hash).to_vec(),
                None => self
                    .pending
                    .iter()
                    .filter(|(h, _)| *h == hash)
                    .map(|(_, i)| *i)
                    .collect(),
            };
            for i in hit {
                if seen.insert(i) {
                    let sugg = &self.words[i as usize];
                    let d = damerau_levenshtein(query, sugg);
                    if d > 0 && d <= max_dist {
                        out.push((d, sugg.as_str()));
                    }
                }
            }
        }
        out
    }
}

/// Every string reachable by deleting between 1 and `max_dist` characters from `word`.
/// Char-aware: user phrase words may contain multi-byte letters (é, ñ, …), so we operate on
/// chars, not bytes — byte-slicing would split a code point and panic on `from_utf8`.
fn deletes_of(word: &str, max_dist: usize) -> Vec<String> {
    let mut out: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut frontier: Vec<String> = vec![word.to_string()];
    for _ in 0..max_dist {
        let mut next: Vec<String> = Vec::new();
        for w in &frontier {
            let chars: Vec<char> = w.chars().collect();
            if chars.len() <= 1 {
                continue;
            }
            for i in 0..chars.len() {
                // Delete the i-th character (char-safe for any UTF-8 input).
                let mut s: String = chars[..i].iter().collect();
                s.extend(&chars[i + 1..]);
                if out.insert(s.clone()) {
                    next.push(s);
                }
            }
        }
        frontier = next;
    }
    out.into_iter().collect()
}

/// Common English inflections of a lowercase alphabetic `word` (≥3 chars): plural/-s,
/// past -ed, progressive -ing (with e-drop and CVC doubling), and -ly/-er/-est. Lets the
/// user's own phrase vocabulary correct its morphology without storing every form.
/// Irregulars (go/went, run/ran) are intentionally not handled.
pub(super) fn inflections(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    if n < 3 {
        return Vec::new();
    }
    let last = chars[n - 1];
    let penult = chars.get(n - 2).copied();
    let consonant_y = last == 'y' && penult.map(|c| !is_vowel(c)).unwrap_or(true);
    // Stem = word minus its last char. Built from chars (not byte-sliced) so multi-byte
    // letters (é, ñ, …) don't panic the slice.
    let stem: String = chars[..n - 1].iter().collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };

    // Plural / 3rd-person singular.
    if matches!(last, 's' | 'x' | 'z') || word.ends_with("ch") || word.ends_with("sh") {
        push(format!("{word}es"));
    } else if consonant_y {
        push(format!("{stem}ies"));
    } else {
        push(format!("{word}s"));
    }

    // Past and progressive (share stem rules).
    if last == 'e' {
        push(format!("{word}d"));
        push(format!("{stem}ing"));
    } else if consonant_y {
        push(format!("{stem}ied"));
        push(format!("{word}ing"));
    } else if is_cvc(&chars) {
        let doubled = format!("{word}{last}");
        push(format!("{doubled}ed"));
        push(format!("{doubled}ing"));
    } else {
        push(format!("{word}ed"));
        push(format!("{word}ing"));
    }

    push(format!("{word}ly"));
    push(format!("{word}er"));
    push(format!("{word}est"));
    out
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Consonant–vowel–consonant ending (last consonant not w/x/y) — signals English
/// final-consonant doubling (run → running, stop → stopped).
fn is_cvc(chars: &[char]) -> bool {
    let n = chars.len();
    if n < 3 {
        return false;
    }
    let (c1, v, c2) = (chars[n - 3], chars[n - 2], chars[n - 1]);
    !is_vowel(c1) && is_vowel(v) && !is_vowel(c2) && !matches!(c2, 'w' | 'x' | 'y')
}

/// Capitalize only the first word of a sentence. `known` gates this to real words so
/// identifiers/code aren't touched. The standalone pronoun "i" is intentionally NOT
/// auto-capitalized: that silently rewrote user input and interfered with spellcheck
/// (every restart it re-fired, since the ignore set is in-memory). Returns the corrected
/// form, or None.
fn capitalize_if_needed(word: &str, prefix: &str, known: bool) -> Option<String> {
    if word.is_empty()
        || !word
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
    {
        return None;
    }
    let all_lower = word.chars().all(|c| !c.is_ascii_uppercase());
    if all_lower && known && at_sentence_start(prefix) {
        let rest: String = word.chars().skip(1).collect();
        let first = word.chars().next().unwrap().to_ascii_uppercase();
        return Some(format!("{first}{rest}"));
    }
    None
}

/// True if `prefix` (buffer content before the current word) sits at a sentence boundary:
/// empty (start of buffer/field), or ending in `.`, `!`, `?`, or a newline.
fn at_sentence_start(prefix: &str) -> bool {
    matches!(
        prefix.trim_end().chars().next_back(),
        None | Some('.' | '!' | '?' | '\n')
    )
}

/// Punctuation stripped from the tail of a typed word before spell-checking, so words like
/// `hello,` or `world.` are still recognized and correctable. Removed from the end only —
/// internal `'` / `-` in `don't` / `well-known` stay intact. A trailing quote char is a
/// closing quotation (or the rare plural possessive `queens'`), so both `'` and `"` strip.
const TRAILING_PUNCT: &[char] = &[',', ';', ':', '_', '-', '/', '?', '!', '.', '\'', '"'];

/// Punctuation stripped from the head of a typed token (opening quotes/brackets typed
/// before the word: `"word`, `(word`, `[word`). Symmetric with `TRAILING_PUNCT` so the
/// pair is preserved verbatim when the correction is re-typed.
const LEADING_PUNCT: &[char] = &['"', '\'', '(', '[', '{', '<', '*', '#'];

/// The token isn't a spell-checkable word: URLs, emails, paths, and alphanumeric jargon
/// (a16z, ISO9001, x86_64) should pass through untouched. Also gates offer-mode: no popup
/// for an email mid-address.
fn is_uncheckable(core: &str) -> bool {
    // any digit disqualifies (version numbers, identifiers, a16z, x86_64)
    if core.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    // URL-ish / path-ish / email-ish tokens: scheme, @, interior dot, or separators
    if core.contains("://")
        || core.contains('@')
        || core.contains("www.")
        || core.matches('.').count() >= 1
        || core.contains('\\')
        || core.contains('/')
    {
        return true;
    }
    false
}

/// Split a typed token for spell-checking. Returns None when the token isn't a checkable
/// word (URLs, emails, paths, digits) — callers skip correction/offer entirely.
/// Returns (leading, core, trailing) where the punctuation wrappers are preserved and
/// re-attached verbatim on correction. A trailing possessive `'s` is folded into the core
/// (`word's` checks `word`, corrects to `fixed's`); `'` alone (Queen's English…
/// `queens'`) stays in the core.
/// The last word of `buffer` (trailing whitespace ignored): its byte start index in
/// `buffer` and its raw token (punctuation wrappers included). Shared by the
/// listener's offer/learn paths and `maybe_correct`.
pub(super) fn current_word(buffer: &str) -> (usize, String) {
    let trimmed = buffer.trim_end();
    let start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
    (start, trimmed[start..].to_string())
}

pub(super) fn split_token(word: &str) -> Option<(&str, &str, &str)> {
    let core_span = word.trim_start_matches(LEADING_PUNCT);
    let lead_len = word.len() - core_span.len();
    let core_tail = core_span.trim_end_matches(TRAILING_PUNCT);
    let mut core = core_tail;
    let mut trailing_len = core_span.len() - core_tail.len();
    // Possessive 's: `word's` → check `word`. Only when what remains is ≥3 chars
    // (don't strip "it's" — contractions are dictionary words themselves).
    if let Some(stem) = core.strip_suffix("'s") {
        if stem.chars().count() >= 3 {
            core = stem;
            trailing_len += 2;
        }
    }
    if core.is_empty() {
        return None;
    }
    if is_uncheckable(core) {
        return None;
    }
    let lead = &word[..lead_len];
    let trailing = &word[word.len() - trailing_len..];
    Some((lead, core, trailing))
}

/// On a word boundary (space): try fuzzy spell correction, then a capitalization pass
/// (sentence-initial capitalization). Returns Some((original_core, corrected)) if a
/// correction was applied, None otherwise. `learned` receives (typo, correction) pairs
/// to persist when the correction sticks (learn-on-accept).
#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_correct(
    buffer: &mut String,
    idx: &Mutex<AbbrevIndex>,
    inj_flag: &Arc<AtomicBool>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
    ignore: &Mutex<std::collections::HashSet<String>>,
    mode: &Mutex<String>,
    learned: &Arc<Mutex<HashMap<String, String>>>,
    db: &Arc<Db>,
    app: &tauri::AppHandle,
) -> Option<(String, String)> {
    let (word_start, raw) = current_word(buffer);
    // Split off punctuation wrappers; None = token isn't a checkable word (URL/email/
    // path/digits) → leave it entirely alone.
    let Some((lead, word, trailing)) = split_token(&raw) else {
        return None; // not a checkable word
    };
    if word.is_empty() {
        return None;
    }
    let prefix = &buffer[..word_start.min(buffer.len())];
    let original = word.to_string();
    let lower = word.to_lowercase();
    let ignored = ignore
        .lock()
        .ok()
        .map(|g| g.contains(&lower))
        .unwrap_or(false);

    // Previous word in the buffer (for bigram context ranking). Stripped of its own
    // punctuation wrappers; skipped when the buffer is empty before this word.
    let prev_word = prefix
        .trim_end()
        .rsplit(' ')
        .next()
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
        .and_then(|p| split_token(&p).map(|(_, core, _)| core.to_string()));

    // 0) Learned corrections: exact-match first — this is the habitual-typo fast path.
    //    Rechecked against the current ignore list (an undone correction also unlearns).
    let learned_hit = if !ignored {
        learned
            .lock()
            .ok()
            .and_then(|g| g.get(&lower).cloned())
            .filter(|c| !c.is_empty())
    } else {
        None
    };
    // One correction pipeline: learned hit → fuzzy → capitalization, first source
    // wins, then a single emission tail.
    let mut correction = learned_hit.and_then(|fixed| {
        let corrected = apply_case(word, &fixed);
        (corrected != word).then_some(corrected)
    });
    // A learned hit is already persisted — skip learn-on-accept for it.
    let from_learned = correction.is_some();

    // 1) Fuzzy correction (only meaningful for ≥3-char words).
    if correction.is_none() && !ignored && word.chars().count() >= 3 {
        let empty = std::collections::HashSet::new();
        let mode_str = mode
            .lock()
            .ok()
            .map(|m| m.clone())
            .unwrap_or_else(|| "normal".to_string());
        correction = idx.lock().ok().and_then(|i| {
            let ig = ignore.lock().ok();
            let ig: &std::collections::HashSet<String> = ig.as_deref().unwrap_or(&empty);
            i.correct_with_mode(word, prev_word.as_deref(), ig, &mode_str)
        });
    }

    // 2) Capitalization (only when the word is otherwise valid and not ignored). Undo of a
    // capitalization adds the word to the ignore list, so it won't be re-applied.
    if correction.is_none() && !ignored {
        let known = idx.lock().ok().map(|i| i.is_known(&lower)).unwrap_or(false);
        correction = capitalize_if_needed(word, prefix, known);
    }

    if let Some(corrected) = correction {
        // Learn-on-accept: persist the typo→correction so the next occurrence is an
        // exact-map hit (no SymSpell traversal, better ranking than the general case).
        if !from_learned && corrected.to_lowercase() != lower {
            if let Ok(mut g) = learned.lock() {
                g.insert(lower.clone(), corrected.to_lowercase());
            }
            let _ = db.add_spellcheck_learned(&lower, &corrected.to_lowercase());
        }
        let prefix_owned = prefix.to_string();
        emit_correction(
            buffer,
            &prefix_owned,
            lead,
            &original,
            &corrected,
            trailing,
            inj_flag,
            last_echo,
            app,
        );
        // Full tokens (with wrappers) — the undo path's erase math needs them.
        let orig_tok = format!("{lead}{original}{trailing}");
        let fixed_tok = format!("{lead}{corrected}{trailing}");
        return Some((orig_tok, fixed_tok));
    }
    None
}

/// Shared correction emission: rewrite the listener buffer and inject the fixed token
/// (with its original punctuation wrappers) off-thread with echo suppression.
#[allow(clippy::too_many_arguments)]
fn emit_correction(
    buffer: &mut String,
    prefix: &str,
    lead: &str,
    original: &str,
    corrected: &str,
    trailing: &str,
    inj_flag: &Arc<AtomicBool>,
    last_echo: &Arc<Mutex<Option<std::time::Instant>>>,
    app: &tauri::AppHandle,
) {
    let raw_chars = super::offer::erase_len(lead, original, trailing);
    let lead = lead.to_string();
    let trailing = trailing.to_string();
    let corrected = corrected.to_string();
    // Update the buffer synchronously (listener state) to reflect the correction.
    *buffer = format!("{prefix}{corrected}{trailing} ");
    // Inject (erase misspelled word + type corrected) off-thread with echo suppression.
    inject_with_suppression(app, inj_flag, last_echo, move |inj| {
        crate::triggers::inject::log_if_err(inj.backspace(raw_chars), "correction erase");
        crate::triggers::inject::log_if_err(
            inj.type_text(&format!("{lead}{corrected}{trailing} ")),
            "correction type",
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damerau_levenshtein_basics() {
        assert_eq!(damerau_levenshtein("hello", "hello"), 0);
        assert_eq!(damerau_levenshtein("teh", "the"), 1); // transposition = distance 1!
        assert_eq!(damerau_levenshtein("kitten", "sitting"), 3);
        assert_eq!(damerau_levenshtein("", "abc"), 3);
    }

    #[test]
    fn split_token_wrappers_and_uncheckable() {
        // trailing punct only
        assert_eq!(split_token("hello,"), Some(("", "hello", ",")));
        assert_eq!(split_token("world!"), Some(("", "world", "!")));
        assert_eq!(split_token("done?!"), Some(("", "done", "?!")));
        // leading punct is now split too (opening quote/bracket before the word)
        assert_eq!(split_token("\"word"), Some(("\"", "word", "")));
        assert_eq!(split_token("(teh"), Some(("(", "teh", "")));
        // both sides
        assert_eq!(split_token("\"recieve\","), Some(("\"", "recieve", "\",")));
        // possessive 's folds into trailing
        assert_eq!(split_token("word's"), Some(("", "word", "'s")));
        // contractions stay intact (they're dictionary words)
        assert_eq!(split_token("don't"), Some(("", "don't", "")));
        // uncheckable: URLs, emails, paths, digits
        assert_eq!(split_token("https://example.com"), None);
        assert_eq!(split_token("user@example.com"), None);
        assert_eq!(split_token("a16z"), None);
        assert_eq!(split_token("x86_64"), None);
        assert_eq!(split_token("ISO9001"), None);
        assert_eq!(split_token("path/to/file"), None);
        assert_eq!(split_token("C:\\dir"), None);
        assert_eq!(split_token("..."), None);
        // plain word
        assert_eq!(split_token("abc"), Some(("", "abc", "")));
    }

    #[test]
    fn deletes_of_generates_all_levels() {
        // distance-1 deletes of "abcd": bcd, acd, abd, abc
        let d1 = deletes_of("abcd", 1);
        assert_eq!(d1.len(), 4);
        assert!(d1.contains(&"bcd".to_string()));
        assert!(d1.contains(&"abc".to_string()));
        // distance {1,2}: 4 one-deletes + 6 two-deletes (every 2-subset) = 10
        let d2 = deletes_of("abcd", 2);
        assert_eq!(d2.len(), 10);
        assert!(d2.contains(&"ab".to_string()));
        assert!(d2.contains(&"cd".to_string()));
        // "ab" yields two distance-1 deletes; distance-2 would be the empty string, skipped.
        let dab = deletes_of("ab", 2);
        assert_eq!(dab.len(), 2);
        assert!(dab.contains(&"a".to_string()) && dab.contains(&"b".to_string()));
    }

    #[test]
    fn deletes_of_handles_multibyte_chars() {
        // Non-ASCII alphabetic words (from user phrases) must not panic on byte-splits.
        // "café" = c,a,f,é(2 bytes). Distance-1 deletes drop one CODE POINT each.
        let d1 = deletes_of("café", 1);
        assert_eq!(d1.len(), 4);
        assert!(d1.contains(&"afé".to_string())); // drop 'c'
        assert!(d1.contains(&"caf".to_string())); // drop 'é'
        assert!(!d1.iter().any(|s| s.contains('\u{fffd}'))); // no replacement chars
                                                             // distance-2 must also be panic-free and yield valid UTF-8.
        for s in deletes_of("café", 2) {
            assert!(std::str::from_utf8(s.as_bytes()).is_ok());
        }
    }

    #[test]
    fn inflections_handle_multibyte_chars() {
        // e-drop / y→ies rules slice the stem; must stay char-safe for non-ASCII.
        let _ = inflections("café"); // ends in 'é' (not e/s/x/z/y) → no panic, plain forms
        let _ = inflections("naïve"); // ends in 'e' → exercises stem slice
        assert!(inflections("naïve").iter().any(|s| s == "naïving")); // {stem}ing, stem char-safe
                                                                      // No malformed output.
        for s in inflections("café") {
            assert!(std::str::from_utf8(s.as_bytes()).is_ok());
        }
    }

    #[test]
    fn symspell_lookups_within_distance() {
        let mut s = SymSpell::new(2);
        for w in ["hello", "help", "world"] {
            s.add(w);
        }
        let words: Vec<&str> = s.lookup("helo", 2).iter().map(|(_, w)| *w).collect();
        assert!(words.contains(&"hello")); // helo→hello: insert l, d=1
        assert!(words.contains(&"help")); // helo→help: o→p, d=1
        assert!(!words.contains(&"world"));
        // exact query never self-matches at distance 0
        assert!(!s.lookup("hello", 2).iter().any(|(d, _)| *d == 0));
        assert!(s.contains("hello") && !s.contains("helo"));
    }

    #[test]
    fn inflections_cover_common_forms() {
        let deploy = inflections("deploy");
        assert!(deploy.contains(&"deploys".to_string()));
        assert!(deploy.contains(&"deployed".to_string()));
        assert!(deploy.contains(&"deploying".to_string()));

        let run = inflections("run");
        assert!(run.contains(&"running".to_string())); // CVC doubling
        assert!(!run.contains(&"ran".to_string())); // irregulars intentionally not handled

        let box_w = inflections("box");
        assert!(box_w.contains(&"boxes".to_string())); // -es after x/z/s
    }

    #[test]
    fn capitalization_only_for_sentence_start() {
        // standalone "i" is left as typed — auto-capitalizing it silently rewrote input
        // and interfered with spellcheck, so it must NOT be corrected anywhere.
        assert_eq!(capitalize_if_needed("i", "", false), None);
        assert_eq!(capitalize_if_needed("i", "hi there ", false), None);
        // sentence-initial: only for known words
        assert_eq!(
            capitalize_if_needed("the", "Hi. ", true),
            Some("The".into())
        );
        assert_eq!(capitalize_if_needed("the", "hi ", true), None); // mid-sentence
        assert_eq!(capitalize_if_needed("The", "Hi. ", true), None); // already capital
        assert_eq!(capitalize_if_needed("github", "Hi. ", false), None); // unknown → leave
        assert_eq!(capitalize_if_needed("", "", true), None);
        assert_eq!(capitalize_if_needed("123", "Hi. ", true), None); // non-alpha
    }

    #[test]
    fn baseline_dictionary_is_loaded() {
        let idx = baseline_index();
        assert!(idx.syms.contains("the"));
        assert!(idx.syms.contains("hello"));
        assert!(idx.syms.contains("don't"));
        assert!(!idx.syms.contains("zzqqxx"));
        assert!(!idx.soundex.is_empty());
        // Regression guard: the bundled list is frequency-trimmed (tier 1 ~25K indexed).
        // Catch a regression to the full ~73K list (memory blowup) or an accidental empty asset.
        let n = idx.syms.words.len();
        assert!(
            (20_000..=30_000).contains(&n),
            "tier-1 dict size {n} outside 20K–30K"
        );
        // Tier 2 (membership-only): non-empty and materially larger.
        assert!(!idx.extra.is_empty());
        assert!(
            idx.extra.len() >= 10_000,
            "tier-2 membership set suspiciously small"
        );
    }

    #[test]
    fn baseline_dict_frequency_order_is_load_bearing() {
        // The SymSpell insertion index doubles as frequency rank — this must hold or
        // ranking silently degrades. Sanity-check the head of the list.
        let idx = baseline_index();
        assert_eq!(idx.syms.rank_of("the"), Some(0), "'the' must be rank 0");
        assert!(idx.syms.rank_of("and").unwrap() < 10);
        assert!(idx.syms.rank_of("hello").unwrap() < 5_000);
        // tier-2 words are known but unranked (rank_of = None)
        let any_extra = idx.extra.iter().next().unwrap();
        assert!(idx.syms.rank_of(any_extra).is_none());
    }

    #[test]
    fn keyboard_weighted_distance_prefers_adjacent_typos() {
        // r↔t adjacent on QWERTY: "arm"→"atm" should cost less than a far substitution of
        // the same integer edit distance. (Both distance 1; weighted ranks "atm" first.)
        let arm_atm = weighted_damerau_levenshtein("arm", "atm");
        let arm_axm = weighted_damerau_levenshtein("arm", "axm"); // r→x far
        assert!(
            arm_atm < arm_axm,
            "adjacent-key substitution must cost less: {arm_atm} vs {arm_axm}"
        );
        assert_eq!(weighted_damerau_levenshtein("teh", "the"), 1.0); // transposition = 1
    }
}
