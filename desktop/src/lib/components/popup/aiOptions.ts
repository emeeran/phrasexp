// aiOptions — parses an AI rewrite into the responses the user can act on.
// Real LLM output rarely comes back as a bare list: it wraps options in a
// preamble ("The original phrasing…"), per-option headings, and rationale
// paragraphs ("Why it works:…"). The popup should show only the responses —
// one per option, ready to insert/copy verbatim.

export type AiOption = {
  /** Heading remainder, e.g. "Clear, professional" — empty for bare list items. */
  label: string;
  /** The insertable response text, with quotes and markdown stripped. */
  text: string;
};

/** "### Option 1: …" / "**Option 2** …" / "Option 3:" — the section headers. */
const HEADING = /^\s*(?:#{1,6}\s+)?\**\s*option\s*\d+\s*[:.)-]?\s*(.*?)\s*\**\s*$/i;
/** Lines that explain an option rather than state it — never part of the response. */
const EXPLAIN = /^\s*(?:[-*+]\s*)?(?:\*\*)?\s*(?:why (?:it|this) works?|rationale|explanation|note:)/i;
const LIST_ITEM = /^(\d+[.)]\s|[-*•]\s)/;

export function splitAiOptions(result: string): AiOption[] {
  const lines = result.split("\n").map((l) => l.trim());

  // A bare list (every line empty or an item) is already just responses.
  const items = lines.filter((l) => LIST_ITEM.test(l));
  if (items.length >= 2 && lines.every((l) => l === "" || LIST_ITEM.test(l))) {
    return items.map((l) => ({ label: "", text: l.replace(LIST_ITEM, "") }));
  }

  // Headed sections: text between the preamble and the rationale, one per header.
  const starts = lines.map((l, i) => (HEADING.test(l) ? i : -1)).filter((i) => i >= 0);
  if (starts.length >= 2) {
    const opts = starts.map((start, k) => {
      const end = k + 1 < starts.length ? starts[k + 1] : lines.length;
      const label = cleanText(lines[start].replace(HEADING, "$1"));
      return { label, text: sectionResponse(lines.slice(start + 1, end)) };
    });
    const real = opts.filter((o) => o.text !== "");
    if (real.length >= 1) return real;
  }

  // Prose, single rewrites, and anything unrecognized stay one blob.
  return [{ label: "", text: result }];
}

/** The response inside one option section: its quoted rewrite, or its first
 *  explanatory-free paragraph. Markdown dressing is stripped — the text must be
 *  paste-ready, since decisions act on it verbatim. */
function sectionResponse(body: string[]): string {
  const lines = body
    .map((l) => l.replace(/^>\s?/, "").trim()) // unwrap blockquotes
    .filter((l) => l !== "" && l !== "*" && l !== "-" && !EXPLAIN.test(l));
  const joined = lines.join("\n");
  // A quoted span is the model showing its literal rewrite — prefer it whole.
  const quoted = joined.match(/"(.+)"/s);
  return cleanText(quoted ? quoted[1] : joined);
}

/** Strip paste-hostile dressing: emphasis markers, backticks, bullet remnants. */
function cleanText(t: string): string {
  return t
    .replace(/^[-*+]\s+/, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*/g, "") // stray emphasis markers (e.g. "**Option 1** — direct")
    .replace(/`/g, "")
    .trim();
}
