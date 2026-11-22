// Shared AI-argument helpers — the frontend mirror of the Rust-side
// param_tokens / substitute_template rules (commands.rs). Pure, unit-tested.

export interface ArgPrompt {
  label: string;
  placeholder: string;
  /** One-tap answers (paraphrase modes, lengths, formats) shown as chips. */
  chips?: string[];
}

/** Every distinct interactive `{token}` in template order — anything beyond
 *  {clipboard}/{selection} (mirrors the Rust-side param_tokens rule:
 *  non-empty, not clipboard/selection, [A-Za-z0-9_] only). */
export function paramTokens(template: string): string[] {
  const out: string[] = [];
  for (const m of template.matchAll(/\{(\w+)\}/g)) {
    if (m[1] !== "clipboard" && m[1] !== "selection" && !out.includes(m[1])) out.push(m[1]);
  }
  return out;
}

/** Prompt copy + preset chips per known token; unknown tokens get a generic ask. */
export const ARG_LABELS: Record<string, ArgPrompt> = {
  language: { label: "translate to", placeholder: "language — e.g. German, Tamil, Japanese…" },
  length: {
    label: "target length",
    placeholder: "e.g. 30 words, 3 bullets, one paragraph…",
    chips: ["30 words", "3 bullets", "one paragraph"],
  },
  format: {
    label: "output as",
    placeholder: "paragraphs or bullet points…",
    chips: ["paragraphs", "bullet points"],
  },
  mode: {
    label: "style",
    placeholder: "e.g. formal, fluent, academic, creative…",
    chips: ["formal", "fluent", "academic", "creative"],
  },
};

/** Label + placeholder for a token, falling back to a generic ask. */
export function argPromptFor(token: string): ArgPrompt {
  return ARG_LABELS[token] ?? { label: token, placeholder: `value for {${token}}…` };
}

// Last-used answers per action (localStorage) — the aiarg prompt pre-fills them so
// running "Paraphrase → academic" again is one Enter.

const LAST_KEY = "aiarg:last";

export function lastParams(actionId: string): Record<string, string> {
  try {
    const all = JSON.parse(localStorage.getItem(LAST_KEY) ?? "{}") as Record<
      string,
      Record<string, string>
    >;
    return all[actionId] ?? {};
  } catch {
    return {};
  }
}

export function rememberParams(actionId: string, params: Record<string, string>) {
  try {
    const all = JSON.parse(localStorage.getItem(LAST_KEY) ?? "{}") as Record<
      string,
      Record<string, string>
    >;
    all[actionId] = params;
    localStorage.setItem(LAST_KEY, JSON.stringify(all));
  } catch {
    // Non-fatal — persistence is a convenience, never a blocker.
  }
}
