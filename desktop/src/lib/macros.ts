// Shared macro helpers used by both the spellcheck/picker Popup and the main Test panel.

import type { InputKind, Phrase } from "./types";

export interface InputField {
  field: string;
  /** Human label shown instead of the field name (cite prompts, mostly). */
  label?: string;
  kind: InputKind;
  options: string[];
}

/** Collect a phrase's inputform field definitions, in body order. A `{cite}`
 *  macro with a blank source joins the prompt once (field `cite`) — the
 *  prompt-once pattern the inputform flow already provides. */
export function fieldsOf(p: Phrase): InputField[] {
  const out: InputField[] = [];
  for (const seg of p.body) {
    if (seg.type === "macro" && seg.macro.type === "inputform") {
      out.push({
        field: seg.macro.field,
        kind: (seg.macro.kind ?? "text") as InputKind,
        options: seg.macro.options ?? [],
      });
    } else if (seg.type === "macro" && seg.macro.type === "cite" && !seg.macro.source.trim()) {
      out.push({ field: "cite", label: "Citation — DOI, URL, or title", kind: "text", options: [] });
    }
  }
  return out;
}

/** Default value for each field: checkbox → "false", select → first option, text → "". */
export function defaultsFor(fields: InputField[]): Record<string, string> {
  const d: Record<string, string> = {};
  for (const f of fields) {
    d[f.field] = f.kind === "checkbox" ? "false" : f.kind === "select" ? f.options[0] ?? "" : "";
  }
  return d;
}
