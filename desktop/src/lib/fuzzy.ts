// Fuzzy subsequence scoring — shared by the search popup and the library list.
// Pure module, no Svelte: unit-tested in fuzzy.test.ts.
//
// Scoring model (higher = better):
//   - a subsequence of the target is REQUIRED (query chars in order); else -1
//   - +8  per consecutive matched char run (contiguity)
//   - +10 per match at a word start (after space/-/_ or camelCase boundary)
//   - +40 abbreviation exact-prefix match (abbr "sig" matches query "sig")
//   - +60 whole-string exact match (case-insensitive)
// Rank ties break by usage count (expanded_count), then name alpha.

export interface Rankable {
  name: string;
  abbreviation?: string | null;
  expanded_count?: number;
  tags?: string[];
}

/** Subsequence match of `query` inside `target` (both lowercased). Returns -1 when
 *  `query` is not a subsequence; otherwise a positive score with positional bonuses. */
export function fuzzyScore(query: string, target: string, startBonus = 10): number {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (q.length === 0) return 0;
  if (q === t) return 100; // exact whole-string match
  if (!isSubsequence(q, t)) return -1;

  let score = 0;
  let ti = 0;
  let prevMatch = -2;
  let runLen = 0; // current consecutive-run length (≥1 when this char is contiguous)
  for (let qi = 0; qi < q.length; qi++) {
    // find next occurrence of q[qi] at or after ti
    while (ti < t.length && t[ti] !== q[qi]) ti++;
    // (isSubsequence already guaranteed a hit)
    const consecutive = ti - 1 === prevMatch;
    const wordStart = ti === 0 || isBoundary(t[ti - 1]);
    runLen = consecutive ? runLen + 1 : 1;
    if (wordStart && runLen === 1) {
      score += startBonus; // this char begins a run at a word boundary
    }
    if (consecutive) score += 12 + runLen * 2; // contiguity beats any positional bonus
    score += 2; // per-char base so longer matches accumulate
    prevMatch = ti;
    ti++;
  }
  // Prefer earlier matches: small penalty for how far in the target we got.
  score -= Math.floor(prevMatch / 8);
  return score;
}

function isSubsequence(q: string, t: string): boolean {
  let ti = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found === -1) return false;
    ti = found + 1;
  }
  return true;
}

function isBoundary(prev: string): boolean {
  return /[\s\-_/.]/.test(prev) || (prev === prev.toLowerCase() && prev !== prev.toUpperCase()) === false;
}

/** Rank phrases for `query`: best fuzzy score across name + abbreviation wins;
 *  ties break by expanded_count desc, then name alpha. `#tag` tokens filter by tag
 *  (AND semantics, case-insensitive, leading `#` optional on the stored form) before
 *  the fuzzy pass; the remaining terms fuzzy-match as usual. Empty query → items
 *  unchanged. Returns at most `limit` items. */
export function rankPhrases<T extends Rankable>(query: string, items: T[], limit = 50): T[] {
  const q = query.trim();
  if (!q) return items.slice(0, limit);
  // Split `#sales sig` into tag filters ["sales"] and free terms ["sig"].
  const tagFilters: string[] = [];
  const terms: string[] = [];
  for (const tok of q.split(/\s+/)) {
    if (tok.startsWith("#") && tok.length > 1) tagFilters.push(tok.slice(1).toLowerCase());
    else if (tok) terms.push(tok);
  }
  if (tagFilters.length > 0) {
    items = items.filter((item) =>
      tagFilters.every((tag) => (item.tags ?? []).some((t) => t.toLowerCase() === tag)),
    );
    if (terms.length === 0) return items.slice(0, limit);
  }
  const scored: { item: T; score: number }[] = [];
  for (const item of items) {
    // Every remaining term must fuzzy-match name or abbreviation (AND).
    let best = Infinity;
    for (const term of terms) {
      const nameScore = fuzzyScore(term, item.name);
      // Exact prefix on the abbreviation is the classic "I typed the abbr" case — big bonus.
      const abbr = (item.abbreviation ?? "").toLowerCase();
      const termLower = term.toLowerCase();
      const abbrScore =
        abbr.length > 0
          ? abbr === termLower
            ? 140
            : abbr.startsWith(termLower)
              ? 100
              : fuzzyScore(term, abbr, 14) > 0
                ? fuzzyScore(term, abbr, 14) + 30
                : -1
          : -1;
      const termBest = Math.max(nameScore, abbrScore);
      if (termBest < 0) {
        best = -1;
        break;
      }
      best = Math.min(best, termBest);
    }
    if (best < 0 || best === Infinity) continue;
    scored.push({ item, score: best });
  }
  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    const ua = a.item.expanded_count ?? 0;
    const ub = b.item.expanded_count ?? 0;
    if (ub !== ua) return ub - ua;
    return a.item.name.localeCompare(b.item.name);
  });
  return scored.slice(0, limit).map((s) => s.item);
}
