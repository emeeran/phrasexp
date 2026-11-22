#!/usr/bin/env python3
"""Generate the bundled spellcheck dictionaries.

Outputs (into desktop/src-tauri/src/triggers/):
  - dict_english.txt       indexed tier: SymSpell deletes + Soundex are built from these.
                           Frequency-sorted (most frequent first) — the loader uses the
                           line number as the word's frequency rank, so order is load-bearing.
  - dict_english_extra.txt membership tier: only consulted for "is this word known", so
                           rare-but-valid words are never flagged as typos. No deletes index.

Inputs:
  - system word list (Debian `american-english`, ~104K entries incl. proper nouns,
    contractions, possessives) — the legitimacy filter.
  - Norvig count_1w.txt (top 333K Google unigrams) — the frequency signal.
    http://norvig.com/ngrams/count_1w.txt

Ranking: exact count_1w hit → its index. count_1w has no apostrophes (it's tokenized on
whitespace), so contractions/possessives rank via a small curated table of the common
contractions ("don't"→"dont" count, etc.) — never via blanket apostrophe-stripping, which
ranked "i's" as highly as "is". Possessives not in the table rank 900_000 (above every
count_1w word but below the 1_000_000 fallback), keeping rare possessive forms out of the
indexed tier while retaining the base words. Everything else → 1_000_000 + alphabetical
position (deterministic fallback, still better than usize::MAX in the Rust loader).

Resource posture: the indexed tier grows the SymSpell delete map ~1.3x vs the old 20K list
(the dominant memory cost — the script prints both counts so drift is visible), while the
membership tier is a plain HashSet (~2MB) that eliminates false positives on rare words.
Words ranked 25K+ are almost never correction targets (they're past the ~99th percentile
of typing), so indexing them buys recall nobody notices.
"""

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT_DIR = REPO / "desktop" / "src-tauri" / "src" / "triggers"
DEFAULT_SYS = Path("/usr/share/dict/american-english")
DEFAULT_COUNTS = Path("/tmp/count_1w.txt")

INDEXED_N = 25_000   # tier 1: deletes + soundex indexed
MEMBER_N = 65_000    # tier 1 + tier 2 total: membership-only capped
MIN_LEN = 3          # the corrector only fires on words >= 3 chars

# Common contractions: ranked by hand from COCA/American Corpus frequency tables rather
# than the stripped count_1w form — count_1w tokenizes on whitespace, so "dont" there is
# mostly the *typo* of "don't" and ranks it ~1500th, far below its true top-60 rank. A
# contraction must outrank its typo-correction competitors or "dont"→"done" wins.
CONTRACTION_RANKS = {
    "don't": 45, "it's": 12, "that's": 60, "i'm": 80, "can't": 110, "won't": 150,
    "i've": 170, "you're": 190, "isn't": 220, "doesn't": 240, "didn't": 260,
    "there's": 280, "i'll": 300, "you'll": 330, "we're": 350, "they're": 370,
    "wasn't": 400, "aren't": 420, "wouldn't": 440, "haven't": 460, "hasn't": 480,
    "couldn't": 500, "shouldn't": 520, "would've": 540, "should've": 560, "could've": 580,
    "weren't": 600, "hadn't": 620, "what's": 640, "let's": 660, "he's": 680,
    "she's": 700, "i'd": 720, "you've": 740, "we'll": 760, "we've": 780, "they've": 800,
    "that'll": 820, "must've": 840, "might've": 860, "o'clock": 880,
}


def total_deletes(words: list[str]) -> int:
    """Distinct delete-KEYS across the whole dictionary (HashMap size in Rust)."""
    keys: set[str] = set()
    for w in words:
        # per-word distinct deletes only; cross-word dedupe via the union below
        keys |= {w[:i] + w[i + 1:] for i in range(len(w)) if len(w) > 1}
    # distance-2 deletes
    lvl2: set[str] = set()
    for w in words:
        if len(w) <= 1:
            continue
        for i in range(len(w)):
            d1 = w[:i] + w[i + 1:]
            if len(d1) <= 1:
                continue
            for j in range(len(d1)):
                lvl2.add(d1[:j] + d1[j + 1:])
    return len(keys | lvl2)


def load_counts(path: Path) -> dict[str, int]:
    counts: dict[str, int] = {}
    with path.open() as f:
        for idx, line in enumerate(f):
            word = line.split("\t")[0].strip().lower()
            if word and word not in counts:
                counts[word] = idx
    return counts


def load_system(path: Path) -> list[str]:
    words: set[str] = set()
    for raw in path.read_text().splitlines():
        w = raw.strip().lower()
        # letters only, plus internal apostrophe (contractions/possessives) and hyphen
        if not w or len(w) < MIN_LEN:
            continue
        ok = all(c.isascii() and (c.isalpha() or c in "'-") for c in w)
        if ok:
            words.add(w)
    return sorted(words)


def main() -> None:
    sys_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SYS
    cnt_path = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_COUNTS
    counts = load_counts(cnt_path)
    system = load_system(sys_path)

    def rank(w: str) -> int:
        if w in CONTRACTION_RANKS:
            return CONTRACTION_RANKS[w]
        if w in counts:
            return counts[w]
        if "'" in w:
            # possessives/unlisted contractions: rare by construction — never blanket-strip
            # (that ranked "i's" as "is" and flooded the top of the indexed tier)
            return 900_000
        return 1_000_000

    # stable sort: rank asc, then alphabetical for the fallback block
    ranked = sorted(system, key=lambda w: (rank(w), w))

    indexed = ranked[:INDEXED_N]
    extra = ranked[INDEXED_N:MEMBER_N]

    old = OUT_DIR / "dict_english.txt"
    if old.exists():
        old_words = [w for w in old.read_text().splitlines() if w.strip()]
        print(f"previous dict: {len(old_words)} words, {total_deletes(old_words)} delete keys")
    print(f"new indexed tier: {len(indexed)} words, {total_deletes(indexed)} delete keys")
    print(f"new membership tier: +{len(extra)} extra words (no deletes index)")

    (OUT_DIR / "dict_english.txt").write_text("\n".join(indexed) + "\n")
    (OUT_DIR / "dict_english_extra.txt").write_text("\n".join(extra) + "\n")

    # sanity: frequency order really is load-bearing
    assert indexed[0] == "the", f"expected 'the' first, got {indexed[0]!r}"
    for probe in ("don't", "the", "and", "hello"):
        assert probe in set(indexed) | set(extra), f"{probe!r} missing from output"


if __name__ == "__main__":
    main()
