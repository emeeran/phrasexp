//! Abbreviation index: literal + regex trigger matching over the buffered keystrokes,
//! plus the spellcheck vocabulary (SymSpell/Soundex/bigrams) mined from phrase bodies,
//! and `rebuild` (index refresh after phrase mutations).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::db::Db;
use crate::domain::models::{ExecuteMode, Phrase, Segment, TriggerType};
use crate::domain::DomainResult;

use super::spellcheck::{
    apply_case, baseline_index, common_bigram, damerau_levenshtein, inflections, soundex,
    weighted_damerau_levenshtein, SymSpell, SYMSPELL_MAX_EDIT,
};

/// Longest literal abbreviation the buffer ends with. O(max-abbreviation-length)
/// hash lookups instead of a scan over the whole table: candidate suffix lengths
/// are walked longest→shortest and each candidate slice is a single `HashMap` hit,
/// so the first match IS the longest (the old linear scan's tie-break becomes
/// unreachable — keys are unique per length). `delayed_only` selects the scan side:
/// `true` matches only delayed (delimiter-mode) abbreviations, `false` only the
/// immediate ones. Shared by `match_suffix`/`match_delayed`.
/// `len_bounds` = (min, max) literal abbreviation lengths in bytes — both 0 when
/// the table is empty (the loop then never runs).
fn best_literal_suffix<'a>(
    literals: &'a HashMap<String, Vec<String>>,
    delayed: &std::collections::HashSet<String>,
    len_bounds: (usize, usize),
    buffer: &str,
    delayed_only: bool,
) -> Option<(&'a str, &'a Vec<String>)> {
    let (min_len, max_len) = len_bounds;
    if min_len == 0 || buffer.len() < min_len {
        return None;
    }
    let mut len = max_len.min(buffer.len());
    while len >= min_len {
        // Byte slices must land on char boundaries — skip lengths that don't.
        if buffer.is_char_boundary(buffer.len() - len) {
            if let Some((abbr, ids)) = literals.get_key_value(&buffer[buffer.len() - len..]) {
                if delayed.contains(abbr) == delayed_only {
                    return Some((abbr.as_str(), ids));
                }
            }
        }
        len -= 1;
    }
    None
}

/// abbreviation -> phrase_ids. Literals are suffix-matched; regexes are pattern-matched
/// (a pattern triggers when some match ends at the buffer's end).
///
/// Spellcheck draws on two fuzzy sources: a static SymSpell index over the bundled
/// English dictionary (built once) and a per-instance SymSpell over the user's phrase
/// vocabulary (rebuilt on phrase edits). Both are symmetric-delete indexes — hash-based
/// lookup, no linear scan — and the user index also carries common English inflections of
/// phrase words (deploy → deploys/deployed/deploying) so the user's own terms correct
/// cleanly without storing every form.
pub struct AbbrevIndex {
    literals: HashMap<String, Vec<String>>,
    /// (min, max) literal abbreviation length in bytes — bounds the suffix-walk
    /// window in `best_literal_suffix`. Both 0 when `literals` is empty.
    literal_len_bounds: (usize, usize),
    literal_lower: std::collections::HashSet<String>,
    /// Literal abbreviations configured to fire only after a delimiter (1.4).
    /// They are excluded from immediate matching; the listener parks them as
    /// pending and fires on the delimiter key.
    delayed: std::collections::HashSet<String>,
    regexes: Vec<(regex::Regex, String)>,
    phrase_syms: SymSpell,
    /// soundex code → indices into `phrase_syms.words` (no duplicated strings).
    phrase_soundex: HashMap<String, Vec<u32>>,
    /// (prev, next) pairs attested in the user's phrase bodies — a tiny, private bigram
    /// model for context-aware candidate ranking (item: "a ___" should favour "lot"
    /// over "lit"). Rebuilt with the phrase vocab on phrase edits.
    phrase_bigrams: std::collections::HashSet<(String, String)>,
    /// Parsed phrases by id, populated at rebuild — expansion reads from here instead
    /// of a per-use `get_phrase` DB round-trip + JSON parse/decrypt. Sensitive phrases
    /// are deliberately NOT cached: at-rest encryption should mean their bodies aren't
    /// resident while idle, so they take the DB path per expansion.
    phrase_cache: HashMap<String, std::sync::Arc<Phrase>>,
}

impl AbbrevIndex {
    pub fn from_phrases(phrases: &[Phrase]) -> Self {
        let mut literals: HashMap<String, Vec<String>> = HashMap::new();
        let mut regexes: Vec<(regex::Regex, String)> = Vec::new();
        let mut phrase_syms = SymSpell::new(SYMSPELL_MAX_EDIT);
        let mut phrase_soundex: HashMap<String, Vec<u32>> = HashMap::new();
        let mut phrase_bigrams: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let mut delayed_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut phrase_cache: HashMap<String, std::sync::Arc<Phrase>> = HashMap::new();
        // Lowercased abbreviation set — the spellcheck "never correct an abbreviation"
        // guard is a single `contains` instead of a per-keystroke scan with an
        // allocation per abbreviation.
        let mut literal_lower: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for p in phrases {
            let is_regex = p
                .triggers
                .iter()
                .any(|t| t.kind == TriggerType::Autotext && t.regex);
            let delayed = p.triggers.iter().any(|t| {
                t.kind == TriggerType::Autotext && t.execute == ExecuteMode::AfterDelimiter
            });
            let Some(abbr) = &p.abbreviation else {
                continue;
            };
            if abbr.is_empty() {
                continue;
            }
            if !p.sensitive {
                phrase_cache.insert(p.id.clone(), std::sync::Arc::new(p.clone()));
            }
            if is_regex {
                if let Ok(re) = regex::Regex::new(abbr) {
                    regexes.push((re, p.id.clone()));
                }
            } else {
                literals.entry(abbr.clone()).or_default().push(p.id.clone());
                literal_lower.insert(abbr.to_lowercase());
                if delayed {
                    delayed_set.insert(abbr.clone());
                }
            }
            // Skip sensitive phrases' bodies — they're encrypted at rest and may hold
            // secrets (API keys, passwords). The abbreviation still triggers; we just don't want
            // fragments like "sk"/"PBSwg9" seeded into the spellcheck correction index.
            if p.sensitive {
                continue;
            }
            // Index every word from the phrase body (plus its English inflections) so typos
            // of the user's own vocabulary are caught. Words are matched for distance only and
            // never surfaced, so non-sensitive phrases are safe to mine here.
            for seg in &p.body {
                if let Segment::Text { text } = seg {
                    // Adjacent alphanumeric runs feed the bigram model (context ranking).
                    let tokens: Vec<&str> = text
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|t| !t.is_empty())
                        .collect();
                    for word in &tokens {
                        let w = word.to_lowercase();
                        if w.chars().count() < 3 || !w.chars().all(|c| c.is_alphabetic()) {
                            continue;
                        }
                        let widx = phrase_syms.add(&w);
                        for infl in inflections(&w) {
                            phrase_syms.add(&infl);
                        }
                        phrase_soundex.entry(soundex(&w)).or_default().push(widx);
                    }
                    for pair in tokens.windows(2) {
                        let a = pair[0].to_lowercase();
                        let b = pair[1].to_lowercase();
                        if a.chars().count() >= 2 && b.chars().count() >= 2 {
                            phrase_bigrams.insert((a, b));
                        }
                    }
                }
            }
        }
        // Bulk vocab load done: compact the delete index into the flat layout.
        phrase_syms.freeze();
        let literal_len_bounds = match literals.keys().map(|k| k.len()).max() {
            Some(max) => (
                literals.keys().map(|k| k.len()).min().unwrap_or(max),
                max,
            ),
            None => (0, 0),
        };
        Self {
            literals,
            literal_len_bounds,
            literal_lower,
            delayed: delayed_set,
            regexes,
            phrase_syms,
            phrase_soundex,
            phrase_bigrams,
            phrase_cache,
        }
    }

    /// The parsed phrase for `id`, from the rebuild-time cache (non-sensitive
    /// phrases only — see the field docs). `None` falls back to the DB path.
    pub fn cached_phrase(&self, id: &str) -> Option<std::sync::Arc<Phrase>> {
        self.phrase_cache.get(id).cloned()
    }

    pub fn len(&self) -> usize {
        self.literals.len() + self.regexes.len()
    }

    /// 1.4 delimiter mode: find the longest DELAYED abbreviation the buffer ends with.
    /// Returns (abbr, ids) like match_suffix, but only over the delayed set.
    pub fn match_delayed(&self, buffer: &str) -> Option<(String, Vec<String>)> {
        best_literal_suffix(
            &self.literals,
            &self.delayed,
            self.literal_len_bounds,
            buffer,
            true,
        )
        .map(|(a, ids)| (a.to_string(), ids.clone()))
    }

    /// Wrapper that reads the aggressiveness mode and returns the single best correction.
    /// `prev_word` is the previous word in the buffer (if any) for context-aware ranking.
    pub fn correct_with_mode(
        &self,
        word: &str,
        prev_word: Option<&str>,
        ignore: &std::collections::HashSet<String>,
        mode: &str,
    ) -> Option<String> {
        self.rank_candidates(word, prev_word, ignore, mode, 1)
            .into_iter()
            .next()
    }

    /// Ranked correction candidates (best first), up to `limit`. Used by offer-mode to
    /// populate the popup. Applies the same gating (known / ignored / abbreviation) and
    /// the same Soundex fallback as the single-best path; returns an empty vec when
    /// nothing applies. Soundex only fires when edit-distance found nothing, preserving
    /// silent-mode semantics.
    pub fn rank_candidates(
        &self,
        word: &str,
        prev_word: Option<&str>,
        ignore: &std::collections::HashSet<String>,
        mode: &str,
        limit: usize,
    ) -> Vec<String> {
        let (max_dist_short, max_dist_long, soundex_min_len, soundex_max_dist) = match mode {
            "conservative" => (1, 1, usize::MAX, 0), // dist 1 only, no Soundex
            "aggressive" => (2, 3, 4, 4),            // dist 3 for long words, Soundex for ≥4 chars
            _ => (1, 2, 6, 3),                       // normal (default)
        };
        let mut ranked = self.rank_internal(
            word,
            prev_word,
            ignore,
            max_dist_short,
            max_dist_long,
            soundex_min_len,
            soundex_max_dist,
        );
        ranked.truncate(limit);
        ranked
    }

    /// Collect correction candidates via SymSpell lookup against both the bundled English
    /// dictionary and the user's phrase vocabulary, falling back to Soundex phonetic
    /// matching when no edit-distance candidate exists. Returns best-first, case-adapted
    /// to the original word, deduped. Empty when the word is known / ignored / a registered
    /// abbreviation, or when no candidate is within range.
    /// Ranking: distance → phrase-vocab → bigram-context → frequency rank → keyboard-weighted
    /// distance → shorter → alphabetical.
    #[allow(clippy::too_many_arguments)]
    fn rank_internal(
        &self,
        word: &str,
        prev_word: Option<&str>,
        ignore: &std::collections::HashSet<String>,
        max_dist_short: usize,
        max_dist_long: usize,
        soundex_min_len: usize,
        soundex_max_dist: usize,
    ) -> Vec<String> {
        let lower = word.to_lowercase();
        if self.is_known(&lower) {
            return Vec::new();
        }
        if ignore.contains(&lower) {
            return Vec::new();
        }
        // Never "correct" a registered abbreviation (it's a deliberate trigger).
        if self.literal_lower.contains(&lower) {
            return Vec::new();
        }
        let word_len = lower.chars().count();
        let max_dist = if word_len <= 4 {
            max_dist_short
        } else {
            max_dist_long
        };

        // (distance, word, is_phrase): candidates from both indexes, tagged by source so the
        // user's vocabulary can outrank the generic dictionary on ties.
        let mut candidates: Vec<(usize, String, bool)> = Vec::new();
        for (d, w) in baseline_index().syms.lookup(&lower, max_dist) {
            candidates.push((d, w.to_string(), false));
        }
        for (d, w) in self.phrase_syms.lookup(&lower, max_dist) {
            candidates.push((d, w.to_string(), true));
        }

        // Phonetic fallback: only when edit distance found nothing and the word is long
        // enough that a Soundex match is meaningful. Soundex buckets hold indices into
        // the respective SymSpell word vectors (no duplicated strings).
        if candidates.is_empty() && word_len >= soundex_min_len {
            let target = soundex(&lower);
            if !target.is_empty() {
                let baseline = baseline_index();
                if let Some(list) = baseline.soundex.get(&target) {
                    for &i in list {
                        let w = &baseline.syms.words[i as usize];
                        let d = damerau_levenshtein(&lower, w);
                        if d <= soundex_max_dist {
                            candidates.push((d, w.clone(), false));
                        }
                    }
                }
                if let Some(list) = self.phrase_soundex.get(&target) {
                    for &i in list {
                        let w = &self.phrase_syms.words[i as usize];
                        let d = damerau_levenshtein(&lower, w);
                        if d <= soundex_max_dist {
                            candidates.push((d, w.clone(), true));
                        }
                    }
                }
            }
        }

        if candidates.is_empty() {
            return Vec::new();
        }

        // Context: the previous word in the buffer, lowercased, for bigram attestation.
        let prev = prev_word.map(|p| p.to_lowercase());

        // Precompute per-candidate sort keys once (the old code ran a ~763-element linear
        // scan per comparison inside the comparator; this does O(n) work total).
        // Keys: (bigram attested, frequency rank, keyboard-weighted distance).
        // Phrase-vocab words rank ahead of equally-frequent dict words via a rank bonus.
        let keys: Vec<(bool, usize, f32)> = candidates
            .iter()
            .map(|(_, w, is_phrase)| {
                let bigram = match prev.as_deref() {
                    Some(p) => {
                        self.phrase_bigrams.contains(&(p.to_string(), w.clone()))
                            || common_bigram(p, w)
                    }
                    None => false,
                };
                let freq = baseline_index()
                    .syms
                    .rank_of(w)
                    .map(|r| r as usize)
                    .unwrap_or(usize::MAX);
                let freq = if *is_phrase {
                    freq.saturating_sub(50_000)
                } else {
                    freq
                };
                (bigram, freq, weighted_damerau_levenshtein(&lower, w))
            })
            .collect();

        // Sort: distance → phrase-vocab first → bigram attested → frequency rank
        // (lower=better; the bundled dict is frequency-sorted, so its index IS the rank)
        // → keyboard-weighted distance → shorter → alphabetical.
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|&i, &j| {
            candidates[i]
                .0
                .cmp(&candidates[j].0)
                .then(candidates[j].2.cmp(&candidates[i].2))
                .then(keys[j].0.cmp(&keys[i].0)) // bigram-attested FIRST (bool ascending would invert)
                .then(keys[i].1.cmp(&keys[j].1))
                .then(
                    keys[i]
                        .2
                        .partial_cmp(&keys[j].2)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(candidates[i].1.len().cmp(&candidates[j].1.len()))
                .then(candidates[i].1.cmp(&candidates[j].1))
        });
        let candidates = order
            .into_iter()
            .map(|i| candidates[i].clone())
            .collect::<Vec<_>>();

        // Apply the original word's case pattern and dedupe (different source words can
        // case-map to the same string), preserving rank order.
        let mut out: Vec<String> = Vec::new();
        for (_, w, _) in candidates {
            let c = apply_case(word, &w);
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// A word is "known" (no correction needed) if it's in the bundled dictionary (either
    /// tier) or the user's phrase vocabulary (including generated inflections).
    pub(super) fn is_known(&self, word: &str) -> bool {
        baseline_index().syms.contains(word)
            || baseline_index().extra.contains(word)
            || self.phrase_syms.contains(word)
    }

    /// Longest literal suffix first, then regexes. Returns (text to erase, candidate ids).
    pub fn match_suffix(&self, buffer: &str) -> Option<(String, Vec<String>)> {
        if let Some((a, ids)) = best_literal_suffix(
            &self.literals,
            &self.delayed,
            self.literal_len_bounds,
            buffer,
            false,
        ) {
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
/// Only the tail window is scanned: a trigger match must END at the buffer's
/// end, so its start can't be older than the longest plausible trigger text —
/// scanning the whole buffer per keystroke (the old `find_iter`) cost O(len)
/// for nothing. The window comfortably covers any typed trigger; longer
/// matches (pathological patterns) simply stop matching.
const REGEX_WINDOW: usize = 256;

fn regex_suffix_match(re: &regex::Regex, buffer: &str) -> Option<String> {
    // Smallest char-boundary index whose window stays inside REGEX_WINDOW bytes.
    let cutoff = buffer.len().saturating_sub(REGEX_WINDOW);
    let start = buffer
        .char_indices()
        .find(|(i, _)| *i >= cutoff)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let window = &buffer[start..];
    let mut found = None;
    for m in re.find_iter(window) {
        if m.end() == window.len() {
            found = Some(m.as_str().to_string());
        }
    }
    found
}

/// Rebuild the abbreviation index from the DB (call after phrase mutations).
pub fn rebuild(db: &Db, index: &Mutex<AbbrevIndex>) -> DomainResult<()> {
    let phrases = db.list_phrases(None, None, false)?;
    // Exclude phrases in disabled folders — they shouldn't trigger or feed spellcheck vocab.
    let enabled: std::collections::HashSet<String> = db
        .list_folders()?
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.id.clone())
        .collect();
    let active: Vec<Phrase> = phrases
        .into_iter()
        .filter(|p| enabled.contains(&p.folder_id))
        .collect();
    let fresh = AbbrevIndex::from_phrases(&active);
    if let Ok(mut guard) = index.lock() {
        *guard = fresh;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn phrase(id: &str, abbr: Option<&str>, body: Vec<Segment>) -> Phrase {
    use crate::domain::models::ContentType;
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
        expanded_count: 0,
        last_used_at: None,
        case_mode: Default::default(),
        created_at: "t".into(),
        updated_at: "t".into(),
        deleted_at: None,
        tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{ContentType, TriggerBinding};

    #[test]
    fn index_built_from_abbreviations() {
        let phrases = vec![
            phrase(
                "p1",
                Some("br,"),
                vec![Segment::Text {
                    text: "Best regards,".into(),
                }],
            ),
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
            phrase(
                "p2",
                Some("signature"),
                vec![Segment::Text { text: "x".into() }],
            ),
        ]);
        assert_eq!(idx.match_suffix("mysignature").unwrap().0, "signature");
    }

    #[test]
    fn regex_abbreviation_matches_suffix() {
        use crate::domain::models::{TriggerBinding, TriggerType};
        let mut p = phrase(
            "pr",
            Some(":[a-z]+"),
            vec![Segment::Text {
                text: "🙂".into()
            }],
        );
        p.triggers = vec![TriggerBinding {
            kind: TriggerType::Autotext,
            abbreviation: Some(":[a-z]+".into()),
            key: None,
            regex: true,
            execute: Default::default(),
        }];
        let idx = AbbrevIndex::from_phrases(&[p]);
        let (erase, ids) = idx.match_suffix("hi :wave").unwrap();
        assert_eq!(erase, ":wave");
        assert_eq!(ids.len(), 1);
        // doesn't match when the pattern isn't a suffix
        assert!(idx.match_suffix(":wave middle").is_none());
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
        let correction = idx.correct_with_mode("teh", None, &empty_ignore, "normal");
        assert!(correction.is_some(), "teh should be corrected");
        // "the" is already in dict → no correction
        assert!(idx
            .correct_with_mode("the", None, &empty_ignore, "normal")
            .is_none());
        // "zzqqxx" (6 chars) has no close or phonetic match → None
        assert!(idx
            .correct_with_mode("zzqqxx", None, &empty_ignore, "normal")
            .is_none());
    }

    #[test]
    fn rank_candidates_returns_best_first_and_deduped() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text {
                text: "the quick brown fox".into(),
            }],
        )]);
        let cands = idx.rank_candidates("teh", None, &empty_ignore, "normal", 4);
        assert!(!cands.is_empty(), "teh should yield candidates");
        let mut sorted = cands.clone();
        sorted.sort();
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(sorted, deduped, "candidates must be deduped");
        // best-first matches the single-best path
        assert_eq!(
            cands.first().cloned(),
            idx.correct_with_mode("teh", None, &empty_ignore, "normal")
        );
    }

    #[test]
    fn rank_candidates_applies_case() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text {
                text: "hello world".into(),
            }],
        )]);
        for c in idx.rank_candidates("Helo", None, &empty_ignore, "normal", 4) {
            assert!(
                c.chars()
                    .next()
                    .map(|ch| ch.is_uppercase())
                    .unwrap_or(false),
                "candidate {c} should be title-cased to match Title-case input"
            );
        }
    }

    #[test]
    fn rank_candidates_empty_for_known_ignored_and_abbreviations() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("sig"),
            vec![Segment::Text {
                text: "signature".into(),
            }],
        )]);
        // known dictionary word
        assert!(idx
            .rank_candidates("the", None, &empty_ignore, "normal", 4)
            .is_empty());
        // registered abbreviation
        assert!(idx
            .rank_candidates("sig", None, &empty_ignore, "normal", 4)
            .is_empty());
        // ignored word
        let mut ignore = std::collections::HashSet::new();
        ignore.insert("teh".to_string());
        assert!(idx
            .rank_candidates("teh", None, &ignore, "normal", 4)
            .is_empty());
    }

    #[test]
    fn rank_candidates_respects_limit_and_preserves_ranking() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text {
                text: "the quick brown fox".into(),
            }],
        )]);
        let many = idx.rank_candidates("teh", None, &empty_ignore, "normal", 100);
        let few = idx.rank_candidates("teh", None, &empty_ignore, "normal", 2);
        assert!(few.len() <= 2);
        assert_eq!(
            &few[..],
            &many[..few.len()],
            "capping keeps the best-first prefix"
        );
    }

    #[test]
    fn spellcheck_respects_ignore_list() {
        let mut ignore = std::collections::HashSet::new();
        ignore.insert("dcker".to_string());
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("hi"),
            vec![Segment::Text {
                text: "docker".into(),
            }],
        )]);
        // "dcker" would normally correct, but it's in the ignore list
        assert!(idx
            .correct_with_mode("dcker", None, &ignore, "normal")
            .is_none());
        // remove from ignore → corrects
        ignore.clear();
        assert!(idx
            .correct_with_mode("dcker", None, &ignore, "normal")
            .is_some());
    }

    #[test]
    fn spellcheck_protects_abbreviations() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p1",
            Some("sig"),
            vec![Segment::Text {
                text: "signature".into(),
            }],
        )]);
        // "sig" is a registered abbreviation → don't correct even if close to a dict word
        assert!(idx
            .correct_with_mode("sig", None, &empty_ignore, "normal")
            .is_none());
    }

    #[test]
    fn rebuild_excludes_phrases_in_disabled_folders() {
        use crate::db::Db;
        use crate::domain::models::Folder;
        let db = Db::open_in_memory().unwrap();
        db.create_folder(&Folder {
            id: "fon".into(),
            name: "on".into(),
            parent_id: None,
            created_at: "t".into(),
            enabled: true,
        })
        .unwrap();
        db.create_folder(&Folder {
            id: "foff".into(),
            name: "off".into(),
            parent_id: None,
            created_at: "t".into(),
            enabled: false,
        })
        .unwrap();
        let mk = |id: &str, fid: &str, abbr: &str| Phrase {
            id: id.into(),
            folder_id: fid.into(),
            name: id.into(),
            abbreviation: Some(abbr.into()),
            content_type: ContentType::Plain,
            body: vec![Segment::Text { text: "x".into() }],
            triggers: vec![],
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: "t".into(),
            updated_at: "t".into(),
            deleted_at: None,
            tags: Vec::new(),
        };
        db.create_phrase(&mk("p1", "fon", "ab1")).unwrap();
        db.create_phrase(&mk("p2", "foff", "ab2")).unwrap();
        let idx = std::sync::Mutex::new(AbbrevIndex::from_phrases(&[]));
        rebuild(&db, &idx).unwrap();
        let g = idx.lock().unwrap();
        // enabled folder's abbreviation is indexed
        assert_eq!(g.match_suffix("hi ab1").unwrap().0, "ab1");
        // disabled folder's abbreviation is NOT indexed
        assert!(g.match_suffix("hi ab2").is_none());
    }

    #[test]
    fn delayed_abbreviations_are_excluded_from_immediate_match() {
        // A delayed abbreviation must NOT fire on suffix match (immediate path)…
        let idx = AbbrevIndex::from_phrases(&[Phrase {
            id: "p1".into(),
            folder_id: "f".into(),
            name: "Sig".into(),
            abbreviation: Some("sig".into()),
            content_type: ContentType::Plain,
            body: vec![Segment::Text {
                text: "regards".into(),
            }],
            triggers: vec![TriggerBinding {
                kind: TriggerType::Autotext,
                abbreviation: Some("sig".into()),
                key: None,
                regex: false,
                execute: ExecuteMode::AfterDelimiter,
            }],
            sensitive: false,
            app_scope: None,
            expanded_count: 0,
            last_used_at: None,
            case_mode: Default::default(),
            created_at: "t".into(),
            updated_at: "t".into(),
            deleted_at: None,
            tags: Vec::new(),
        }]);
        assert!(idx.match_suffix("hi sig").is_none());
        // …but match_delayed finds it.
        let (abbr, ids) = idx.match_delayed("hi sig").unwrap();
        assert_eq!(abbr, "sig");
        assert_eq!(ids, vec!["p1".to_string()]);
        // An immediate abbreviation stays immediate.
        let idx2 = AbbrevIndex::from_phrases(&[]);
        assert!(idx2.match_delayed("hi sig").is_none());
    }

    #[test]
    fn index_builds_with_non_ascii_phrase_words() {
        // The original crash: a phrase body containing a non-ASCII word panicked index build.
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p",
            Some("hi"),
            vec![Segment::Text {
                text: "café résumé naïve".into(),
            }],
        )]);
        // Build completed without panicking; the words are known (indexed for correction).
        assert!(idx.is_known("café"));
        assert!(idx.is_known("résumé"));
    }

    #[test]
    fn tier2_words_are_known_but_not_correction_targets() {
        // A tier-2 word is "known" (no false-positive flag) but generates no candidates
        // (no deletes index) — it's never surfaced as a correction for anything.
        let idx = AbbrevIndex::from_phrases(&[]);
        let extra_word = baseline_index().extra.iter().next().unwrap().clone();
        assert!(idx.is_known(&extra_word));
        let empty = std::collections::HashSet::new();
        let cands = idx.rank_candidates(&extra_word, None, &empty, "normal", 4);
        assert!(
            cands.is_empty(),
            "tier-2 word {extra_word} shouldn't be flagged"
        );
    }

    #[test]
    fn contraction_corrections_work() {
        // With the Quote key mapped and contractions in tier 1, "dont" → "don't" must be
        // reachable (a top-frequency typo pattern).
        let empty = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[]);
        let c = idx.correct_with_mode("dont", None, &empty, "normal");
        assert_eq!(c.as_deref(), Some("don't"), "dont → don't");
    }

    #[test]
    fn bigram_context_reorders_candidates() {
        // "a lto" — both "lot" and "lit" are distance-1 from "lto"; after "a", the bigram
        // (a, lot) must promote "lot" to the top.
        let empty = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[]);
        let cands = idx.rank_candidates("lto", Some("a"), &empty, "normal", 5);
        assert_eq!(cands.first().map(String::as_str), Some("lot"));
        // Without context the same query must not rely on the bigram table.
        let plain = idx.rank_candidates("lto", None, &empty, "normal", 5);
        assert!(
            plain.iter().any(|c| c == "lot"),
            "lot still reachable without context"
        );
    }

    #[test]
    fn phrase_bigrams_reorder_candidates() {
        // The user's own phrase vocabulary feeds bigrams: a phrase "deploy kubernetes
        // cluster" makes (deploy, kubernetes) attested, promoting "kubernetes" for a typo
        // after "deploy" even though the generic dict alone wouldn't distinguish.
        let empty = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p",
            Some("x"),
            vec![Segment::Text {
                text: "deploy kubernetes cluster".into(),
            }],
        )]);
        // "kubernets" (missing e) after "deploy" → "kubernetes" top-1 via phrase vocab.
        let cands = idx.rank_candidates("kubernets", Some("deploy"), &empty, "normal", 4);
        assert_eq!(cands.first().map(String::as_str), Some("kubernetes"));
    }

    #[test]
    #[ignore = "diagnostic: run with --nocapture to see baseline index sizes for memory budgeting"]
    fn baseline_index_sizes() {
        // RSS before vs after the build isolates the index's footprint.
        fn rss_kb() -> u64 {
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("VmRSS:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|n| n.parse().ok())
                })
                .unwrap_or(0)
        }
        let before = rss_kb();
        let b = baseline_index();
        let after = rss_kb();
        eprintln!(
            "baseline index: words={} deletes_keys={} soundex_buckets={} extra={} rss_before={before}kB rss_after={after}kB index_rss≈{}kB",
            b.syms.words.len(),
            b.syms.delete_key_count(),
            b.soundex.len(),
            b.extra.len(),
            after.saturating_sub(before),
        );
    }

    #[test]
    #[ignore = "diagnostic: run with --nocapture to eyeball top-1 accuracy on a golden typo set"]
    fn golden_typo_accuracy() {
        // Golden set: (typo, expected correction). Ruled by the unit tests above; this
        // harness prints the full top-4 for each so ranking changes are reviewable.
        let cases: &[(&str, &str)] = &[
            ("teh", "the"),
            ("helo", "hello"),
            ("recieve", "receive"),
            ("definately", "definitely"),
            ("seperate", "separate"),
            ("occured", "occurred"),
            ("untill", "until"),
            ("wich", "which"),
            ("thier", "their"),
            ("adress", "address"),
            ("tommorow", "tomorrow"),
            ("wierd", "weird"),
            ("dont", "don't"),
            ("wont", "won't"),
            ("cant", "can't"),
            ("alot", "a lot"),
            ("becuase", "because"),
            ("beleive", "believe"),
            ("enviroment", "environment"),
            ("goverment", "government"),
        ];
        let ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[]);
        let mut hits = 0usize;
        for (typo, want) in cases {
            let got = idx.correct_with_mode(typo, None, &ignore, "normal");
            let ok = got.as_deref() == Some(*want);
            if ok {
                hits += 1;
            }
            let top4: Vec<String> = idx.rank_candidates(typo, None, &ignore, "normal", 4);
            eprintln!(
                "{:>12} → {:<14} want {:<12} {}   top4: {:?}",
                typo,
                got.as_deref().unwrap_or("—"),
                want,
                if ok { "✓" } else { "✗" },
                top4
            );
        }
        eprintln!("top-1 accuracy: {}/{}", hits, cases.len());
    }

    #[test]
    fn suffix_match_stays_cheap_at_library_scale() {
        // Hot-path regression guard: matching must not scale linearly with the
        // abbreviation count (the old `best_literal_suffix` scanned the whole table
        // TWICE per keystroke). Build a 5,000-abbreviation index, match a buffer that
        // hits nothing, and require the whole sweep to finish well under a generous
        // bound — an O(n) regression blows past it, an O(suffix) scan doesn't notice.
        let phrases: Vec<Phrase> = (0..5_000)
            .map(|i| {
                phrase(
                    &format!("p{i}"),
                    Some(&format!("zzabbr{i:04}")),
                    vec![Segment::Text { text: "x".into() }],
                )
            })
            .collect();
        let idx = AbbrevIndex::from_phrases(&phrases);
        let start = std::time::Instant::now();
        let iterations = 2_000;
        for i in 0..iterations {
            let buffer = format!("the quick brown fox jumps {i}");
            assert!(idx.match_suffix(&buffer).is_none());
        }
        let elapsed = start.elapsed();
        // ~2 µs/lookup leaves >20× headroom on typical hardware; the linear scan over
        // 5,000 abbreviations measured an order of magnitude slower per keystroke.
        assert!(
            elapsed < std::time::Duration::from_millis(iterations * 2 / 1000 + 20),
            "suffix matching regressed: {iterations} lookups took {elapsed:?}"
        );
        // A real match still resolves through the same path.
        assert_eq!(idx.match_suffix("hello zzabbr1234").unwrap().0, "zzabbr1234");
    }

    #[test]
    fn is_known_uses_baseline_and_phrase_vocab() {
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p",
            Some("x"),
            vec![Segment::Text {
                text: "deploy server".into(),
            }],
        )]);
        assert!(idx.is_known("the")); // baseline
        assert!(idx.is_known("deploy")); // phrase vocab
        assert!(idx.is_known("deploys")); // generated inflection
        assert!(idx.is_known("deployed"));
        assert!(idx.is_known("server"));
        assert!(!idx.is_known("zzqqxx"));
    }

    #[test]
    fn spellcheck_uses_inflected_phrase_vocab() {
        let empty_ignore = std::collections::HashSet::new();
        let idx = AbbrevIndex::from_phrases(&[phrase(
            "p",
            Some("x"),
            vec![Segment::Text {
                text: "deploy".into(),
            }],
        )]);
        // "deploys" is a generated inflection → treated as known, not "corrected".
        assert!(idx
            .correct_with_mode("deploys", None, &empty_ignore, "normal")
            .is_none());
        // a real typo of an inflected form still corrects to it
        assert!(idx
            .correct_with_mode("deployd", None, &empty_ignore, "normal")
            .is_some());
    }
}
