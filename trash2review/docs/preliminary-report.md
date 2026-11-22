# PhraseXp Preliminary Competitive Report

**Date:** 2026-10-27
**Scope:** PhraseXp (Tauri 2 desktop app, v1.4.x) vs. PhraseExpress Pro, Grammarly, QuillBot, and comparable tools (TextExpander, Espanso, aText, Text Blaze).
**Purpose:** Identify what to keep, what to fix, and which competitor features are worth integrating.

---

## Methodology & Limitations

The application itself was inventoried directly from source (Rust core `desktop/src-tauri/`, ~14.5k LOC; Svelte 5 UI `desktop/src/`), cross-checked against `desktop/HOW_TO_USE.md` and the test suite (~207 Rust unit tests, 1 Vitest file).

An REA (reverse-engineering) analysis of the competitor products was **not possible in this environment**: no local artifacts for PhraseExpress Pro (Windows shareware), Grammarly, or QuillBot exist on this machine, and Grammarly/QuillBot are primarily web services rather than analyzable binaries. Competitor feature sets below are grounded in current vendor documentation and 2026 third-party comparisons (sources listed at the end). If binary/ASAR artifacts of any competitor are obtained later, an REA pass can deepen this report (e.g., decompiling the PhraseExpress Windows binary to study its macro-function surface, or analyzing Grammarly's desktop Electron ASAR for its overlay/injection approach).

---

## 1. Strengths of PhraseXp to Retain

These are differentiators or well-executed foundations; changes should not regress them.

| # | Strength | Why it matters / where it lives |
|---|----------|--------------------------------|
| S1 | **Local-first + encrypted at rest** — SQLite with AES-256-GCM, DEK in the OS keychain (`enc1:` envelopes; `db/repo.rs`). None of the cloud competitors (TextExpander, Grammarly) offer this posture. | Privacy is the core brand pitch vs. TextExpander/PhraseExpress cloud sync. |
| S2 | **BYO AI with OpenAI-compatible endpoints** (Groq default, self-hosted friendly) plus a full selection-capture AI flow: action chooser → result with Replace / Insert / Copy / Retry / Discard, `{clipboard}`/`{selection}`/`{language}` templating, global "brand voice" instructions, inline template editing. | Closest thing on the market to "PhraseExpress AI macros, but local and provider-agnostic." Retain the pending-state `ai_decide` design (commands.rs). |
| S3 | **Broad macro engine — 18 macro kinds** (datetime, datemath, form input, random, math, clipboard, nested, env, pick, file, image, ai, aiaction, uuid, shell, counter, delay, cursor) with a streamed `OutChunk` model supporting delays and caret repositioning. | Already rivals PhraseExpress's macro breadth for a v1; the segment-stream architecture is the right foundation to extend. |
| S4 | **Integrated spellcheck with learning** — bundled 65K dictionary, SymSpell + Damerau-Levenshtein + Soundex, learned-typo table, undo via Left-arrow, three aggressiveness levels, per-app exclusion. | Grammarly-class "as you type" ambition, fully offline. Unique among text expanders. |
| S5 | **Real GUI** — three-pane library, fuzzy search, drag-resizable panes, macro picker, test panel, popup with 7 modes (search, form inputs, choose, offer, ai, aiarg, airesult). | Espanso's biggest weakness is precisely the lack of this; keep investing here. |
| S6 | **Migration-friendly importers** — PhraseXp JSON, CSV, AHK, aText, TextExpander, Espanso; plus version history (50 snapshots/phrase), duplicate finder, usage stats. | Lowers switching cost from every major competitor — a genuine go-to-market asset. |
| S7 | **Robust trigger model** — autotext with case adaptation (adapt/always/never), after-delimiter or immediate firing, regex triggers, per-phrase hotkeys with Wayland listener fallback, app-scoping by window title. | The trigger bindings data model (`TriggerBinding`) is extensible; keep it. |
| S8 | **Clipboard history with save-as-phrase** and Markdown→rich-text paste. | Covers the CopyQ/PasteBar niche inside the same tool. |
| S9 | **Practical deployment details** — tray/minimized start, autostart, single instance, portable mode, exclusion list, sensitive-phrase flag. | Table-stakes for daily-driver software, already done. |
| S10 | **Strong test culture in the core** (74 autotext tests, 43 repo tests, crypto/expand/inject/ai covered). | Protects the hard, subtle logic. Retain discipline as the macro/AI surface grows. |

---

## 2. Weaknesses, Frustrations, and Gaps

### Platform & injection reliability (highest user pain)
- **W1. US keyboard-layout assumption** on all platforms — non-US layouts will mis-inject characters. Blocking for any non-US adoption.
- **W2. Wayland is second-class:** listener requires raw evdev + `input` group; global shortcuts and app-scoping (xdotool) are X11-only; ydotool must be running for injection (`inject.rs`). On a default Ubuntu 24 Wayland session, several marquee features degrade.
- **W3. macOS gaps:** no keyboard grab in the popup, Secure Input pauses listening; Windows elevated windows ignore synthetic input. Documented, but not solved.

### Missing competitive table-stakes
- **W4. No sync / multi-device story at all.** TextExpander and PhraseExpress both sell cross-device sync; JSON export/import is the only transfer path. Even a file-based (Syncthing/WebDAV/S3) sync would fit the local-first ethos.
- **W5. No team sharing.** Shared snippet groups are TextExpander's and PhraseExpress Network's core B2B feature; PhraseXp is strictly single-user local.
- **W6. No mobile companion** (PhraseExpress has a free iOS app; TextExpander syncs iOS). Lower priority for a desktop-first product but a visible gap.
- **W7. Rich text is latent, not exposed.** `ContentType` parses `richtext/html/word` in the repo layer, but the UI only offers Plain/Markdown — and Markdown delivery clobbers the clipboard.
- **W8. No statistics/insights.** PhraseXp tracks `expanded_count` and top-5 usage internally but surfaces almost none of it; TextExpander makes expansion stats a headline value prop.

### AI feature gaps vs. Grammarly/QuillBot
- **W9. No inline/ambient assistance.** PhraseXp AI is pull-based (hotkey → chooser → result). Grammarly's moat is continuous, in-place suggestions with tone detection; PhraseXp never proactively helps.
- **W10. No paraphrase modes, tone rewrites, summarization, or translation presets.** QuillBot's 10 paraphrase styles and Grammarly's tone adjustments map naturally onto PhraseXp's existing builtin-AI-action list, but only a fraction exist.
- **W11. No grammar/style engine of its own** beyond spellcheck — no grammar rules, clarity, conciseness, or passive-voice detection (all offline-feasible with LanguageTool-style approaches or local models).
- **W12. No streaming AI results** in the popup — result appears only after the full response, which feels slow next to any modern AI UI.

### Product/engineering hygiene
- **W13. Frontend test coverage is one file** (`fuzzy.test.ts`); no component or E2E tests despite a 7-mode popup and a complex editor.
- **W14. Schema duplication risk:** the repo layer re-implements migration SQL inline ("Same schema as migrations/0001_init.sql") — drift waiting to happen.
- **W15. Docs lag code** (HOW_TO_USE lists 11 macro kinds; code has 18) and omit uuid/shell/counter/image macros.
- **W16. No macro recorder.** PhraseExpress's visual macro recorder is a flagship feature; PhraseXp macros must be composed by hand.
- **W17. Clipboard clobbering** on Markdown delivery and approximate cursor restoration on paste are recurring papercuts.

---

## 3. Suggested Feature Integrations

Prioritized: impact × fit with the local-first architecture. "From" names the primary donor product.

### Tier 1 — High impact, natural fit
1. **Paraphrase mode set as builtin AI actions** *(from QuillBot)* — Fluency / Formal / Casual / Concise / Expand / Academic presets on top of the existing `ai_actions` + selection-capture flow. Cheap to ship (they're prompt presets), instantly competitive with QuillBot's core.
2. **Tone detection & tone rewrite** *(from Grammarly)* — an "Detect tone" builtin action plus tone-targeted rewrites ("rewrite friendlier / more professional"). Reuses `{selection}` pipeline and `ai_instructions` brand voice.
3. **Streaming AI results in the popup** *(from Grammarly/QuillBot UX)* — stream tokens into `PopupAiResult.svelte`; Replace/Insert act on partial or final text. The `ai_decide` pending-state design already supports this.
4. **File-based sync** *(from TextExpander, reinterpreted local-first)* — sync folder exporting/importing via a user-chosen directory (Syncthing/WebDAV/git), with last-writer-wins and the existing version history as the conflict safety net. No cloud server required.
5. **Summarizer action with length controls** *(from QuillBot)* — summarize selection to N words / bullet points; natural `aiarg`-style parameter popup.

### Tier 2 — High impact, more work
6. **Fill-in form improvements** *(from PhraseExpress)* — PhraseXp has text/select/checkbox inputs; add date pickers, dropdowns populated from files, per-field defaults, and form memory (reuse last values). PhraseExpress's `%fill%` forms are its most-copied feature.
7. **Macro recorder (Phase 1: keystroke recorder)** *(from PhraseExpress)* — record key sequences into a delay/key `OutChunk` stream. Phase 2 could add window targeting.
8. **Expansion statistics dashboard** *(from TextExpander)* — `expanded_count`/`last_used_at` already exist; add weekly expansion counts, "time saved" estimate (chars × avg typing speed), and per-folder charts in Settings or the status bar.
9. **Inline autocomplete suggestions** *(from PhraseExpress AutoComplete / Grammarly)* — as the user types, suggest phrase completions in a subtle ghost-text or minimal popup, per-app excludable. The autotext buffer machinery already tracks typing; this is a UI delivery mode, not a new engine.
10. **Rich-text phrase authoring** *(from PhraseExpress)* — finish the latent `richtext/html` path: render Markdown to HTML and paste richly **without clobbering the clipboard** (save/restore via `arboard`), and expose HTML content type in the editor.

### Tier 3 — Strategic / differentiating
11. **Local grammar & style checking** *(from Grammarly, reinterpreted offline)* — extend SymSpell infrastructure with a rule engine (repeated words, double spaces, capitalization, common confusions), optionally an on-device model for grammar rewrites. "Grammarly that never leaves your machine" is a defensible niche no major competitor occupies.
12. **Snippet sharing via exportable signed packs** *(from TextExpander teams)* — team distribution without a server: shared folders exported as versioned packs others import/update. Pairs with the existing importer suite.
13. **PhraseExpress-style macro function set** — phrase nesting links, conditional logic, and external file/CSV data sources as new macro kinds feeding the `OutChunk` stream.
14. **Translation action** *(from QuillBot)* — another `aiarg` preset (`{language}` already exists end-to-end).

### Explicit non-goals (recommended)
- Cloud account/subscription backend — contradicts the local-first identity and the encrypted-at-rest differentiator (S1).
- Plagiarism/AI-detector tools *(QuillBot)* — wrong product shape, heavy backend dependence.

---

## 4. Quick-Win Sequence (suggested)

1. Tier-1 items 1, 2, 5 (prompt presets over the existing AI pipeline — days, not weeks).
2. Streaming popup results (3) — contained UI/backend change.
3. Stats dashboard (8) — data already collected.
4. File-based sync (4) — biggest strategic gap (W4), moderate complexity.
5. Rich-text paste without clipboard clobber (10) + fix W17 papercuts.

---

## Sources

- [PhraseExpress — Autotext / Text Expander](https://www.phraseexpress.com)
- [PhraseExpress AutoComplete docs](https://www.phraseexpress.com/doc/settings/autocomplete)
- [Bartels Media — Using AI in any program as-you-type (v17)](https://www.bartelsmedia.com/blog/tutorials/using-ai-in-any-program-as-you-type)
- [Grammarly Product Features](https://www.grammarly.com/features)
- [QuillBot review (2026), selfpublishing.com](https://selfpublishing.com/quillbot-review)
- [Best Text Expansion Tools for Mac in 2026 — Timing](https://timingapp.com/blog/text-expansion-tools-mac)
- [TextExpander Organization / team sharing](https://textexpander.com/learn/organization/members)
- [TextExpander Business](https://textexpander.com/business)
