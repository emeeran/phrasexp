# Task List: Writing-Quality Features + UI/UX

Companion to `scope-writing-quality-uiux.md`. Ordered by the suggested sequence;
check off as you go. Effort: S ≤ 2 days, M ~ a week.

## Phase 0 — Safety net (before touching hot files)

- [x] Add `vitest.config.ts` (currently missing; only `fuzzy.test.ts` exists)
- [x] Characterization tests: Popup mode state machine (`Popup.svelte` mode union, `paramToken()`) — extracted to `$lib/aiArgs.ts`, tested
- [x] Characterization tests: `PhraseEditor.svelte` derived state / save payload shape — extracted to `$lib/editor/phraseDraft.ts`, tested
- [x] Snapshot or unit tests for template substitution (`substitute_template`, frontend `paramToken()` mirror) — Rust tests extended (multi-occurrence locked)

## Phase 1 — Refactor hot files (B1)

- [x] Split `Popup.svelte` (654 lines): one component per mode (`search|inputs|choose|offer|ai|aiarg|airesult`), typed mode-state module — *mode components already existed; orchestrator keeps the event wiring by design (rdev listeners + inject sequencing)*
- [x] Split `PhraseEditor.svelte` (743 lines): child components for triggers / macros / AI sections → `TriggerFields` / `MacroFields` / `AiActionInlineEdit` / `VariationsPanel`
- [x] Rust: split `autotext.rs` (3,555 lines) into `listen` / `spellcheck` / `offer` modules — no behavior change, `cargo test` must stay green (into listen/spellcheck/index/expand/offer; zero-deletion line-multiset diff)
- [x] Frontend pattern consolidation (B3): extract `createResource` (skeleton+`$effect` loading), `useDraft` (settings draft-copy/save), shared error-toast handler; migrate existing components

## Phase 2 — Paraphrase modes + summarizer (A1 + A3)

- [x] Extend `substitute_template` / `paramToken()` to support multiple interactive tokens (keep unit tests green, add new ones) — Rust `param_tokens`/`required_params` + JS `paramTokens`
- [x] Add paraphrase builtins (formal / fluent / academic / creative) to `BUILTIN_AI_ACTIONS` via the `MERGED_AI_ACTIONS` / `RETEMPLATED_AI_ACTIONS` convention — formal/fluent/academic already existed; added `ai-creative` + `ai-paraphrase` (`{mode}` token)
- [x] Mode chips in `PopupAiArg.svelte` (mode pre-fills the token param)
- [x] Persist last-used mode per action (settings or localStorage)
- [x] Summarizer polish: `{length}` + bullets-vs-paragraph tokens (`{format}` added via chained retemplate); scrollable long-output handling in `PopupAiResult.svelte` (already had a 320px scroll region inside the adaptive window)
- [x] Update `HOW_TO_USE.md` §5 AI presets + quick reference

## Phase 3 — Accessibility pass (B2, on the refactored popup)

- [x] `aria-live` region on streaming `ai-delta` output in `PopupAiResult.svelte`
- [x] `aria-live` on toasts (toast store) — *already present (`role="status"` region)*
- [x] Focus trap in `common/Modal.svelte` (+ Escape/restore-focus behavior) — *already present*
- [x] Convert `PhraseList` / `FolderTree` rows to semantic buttons or listbox/option roles with keyboard support — *already present (listbox/option + buttons)*
- [x] DOM keydown handling alongside rdev-routed `offer-key` events in popup modes that hold focus — *already the design: offer mode is rdev-only (X grab can't deliver DOM keys), all other modes handled by `svelte:window` keydown*
- [x] Theme parity audit (B4): complete light/sepia token blocks in `app.css`; fix dark-only overrides — *token blocks already complete; added `color-scheme` per theme (native controls followed dark UA styling on every theme)*

## Phase 4 — Citations (A6)

- [x] Rust command `fetch_citation_metadata` (Crossref/DOI via existing `reqwest`; no new capabilities)
- [x] Citation formatting: APA / MLA / Chicago from structured metadata (template code, AI fallback) — deterministic template code; AI fallback deliberately omitted
- [x] Add `cite` variant to `Macro` enum + `evaluate_macro` / `Db::expand` handling
- [x] Citation input popup mode (reuse `aiarg` prompt-once pattern) — implemented via the inputs popup: blank `source` collects field `cite` like an inputform field
- [x] Tests: metadata parsing + style formatting (pure functions)
- [x] `HOW_TO_USE.md`: new macro in §5 gallery

## Phase 5 — On-demand grammar check (A2, explicit only)

- [x] "Fix grammar" streaming AI action wired to a selectable-text hotkey (reuse `start_selection_ai` flow) — *already wired: `ai-grammar` builtin + selection hotkey + streaming result popup*
- [x] Diff preview verified for grammar edits in `PopupAiResult.svelte` — `ai-result` now carries the original selection; a diff toggle renders char-level deletions/insertions
- [x] Settings: opt-in "review sentence" offer-popup variant (off by default; per-app exclusions apply)

## Backlog — deferred deliberately

- [ ] While-typing sentence grammar — only as loud, opt-in, per-app-scoped setting (see scope doc A2)
- [ ] Plagiarism detection — needs cloud search index; contradicts local-first
- [ ] AI-content detection — weak value, unreliable
- [ ] i18n (B6) — when non-English users exist
- [ ] Retired Groq preset cleanup (`mixtral-8x7b-32768` in `AiPage.svelte`) — fold into any Phase 2+ AI-page work
- [ ] Offer-popup hardcoded sizing → ResizeObserver pattern (B5) — fold into popup work if users report issues
