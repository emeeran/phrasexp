# Scope: Writing-Quality Features + UI/UX Improvements

Grounded in the actual codebase (v1.7.0). Effort ratings assume one dev familiar with the repo: **S** ≤ 2 days, **M** ~ a week, **L** multi-week.

---

## Part A — Writing-quality features

### A1. Paraphrase modes (formal / fluent / academic / creative) — **S–M** ✅ recommended first

80% of the machinery exists: the `AiAction` template system (`domain/models.rs:212`) with `{clipboard}` + one interactive `{token}` placeholder, streaming selection-AI (`run_ai_action` → `ai-delta` events → result popup with replace/insert/retry), and a builtin "make formal" action.

**Scope:**
- Add 3–4 paraphrase builtins (or one `paraphrase` action with a mode token) to `BUILTIN_AI_ACTIONS` — mind the `MERGED_AI_ACTIONS`/`RETEMPLATED_AI_ACTIONS` convention so user edits aren't clobbered.
- UI: mode chips in `PopupAiArg.svelte` (mode = the `{token}` param, pre-filled) — no Rust change needed if modes are just preset params.
- Nice-to-have: remember last-used mode per action.

**Gap to note:** templates allow only **one** interactive token — a paraphrase action wanting `{tone}` + `{length}` needs the `substitute_template` / `paramToken()` logic (commands.rs ~660, Popup.svelte:245) extended to multi-token. The only real code change, and it's pure + unit-tested.

### A2. Sentence-level grammar checking — **M (on-demand) / L (while-typing)** ⚠️ split this

Two very different scopes under one name:

- **On-demand grammar check (M):** select text → hotkey → "Fix grammar" as a streaming AI action. Works today almost for free; the diff preview in `PopupAiResult.svelte` already shows what changed. **Recommend shipping this.**
- **Real-time while-typing grammar (L + a philosophy problem):** the spelling engine (`autotext.rs`, SymSpell/Damerau, all local) corrects word-level typos at keystroke latency. Sentence-level grammar can't be done locally without shipping a language model or a LanguageTool server (Java — kills the lightweight local-first story). The alternative is sending every sentence to the AI provider *silently* — breaks the "network only for AI you explicitly enable" promise (HOW_TO_USE.md §17) and adds 300ms–2s latency mid-typing.

**Recommendation:** keep silent spellcheck local; add grammar as *explicit* actions (selection hotkey, plus a "review sentence" offer-popup variant that appears only when the user opts in). If real-time is ever wanted, gate it behind a loud, per-app-scoped, opt-in setting.

### A3. Summarizer — **S** ✅ mostly done

A "summarize" builtin already exists. Remaining scope is UX: length/format tokens (`{length}`, bullets-vs-paragraph), a dedicated card in the AI result popup for long inputs (popup clamps at 720px height — long summaries need scrolling), and maybe summarize-from-file via the existing `file` macro. Near-zero Rust work.

### A4. Plagiarism detection — **L, doesn't fit the product** ❌ recommend deferring

QuillBot's plagiarism checker is a cloud service with a web-scale search index. Doing it for real needs a search API integration (per-query cost, rate limits, new privacy surface: user text sent to a third party on every check). No credible local implementation exists; a weak version is worse than none. **Defer, or expose as "check via external service" with a data-leaves-your-machine warning — which contradicts local-first positioning.**

### A5. AI-content detection — **M–L** ⚠️ weak value

Detector APIs exist (GPTZero etc.) but accuracy is contested and it's the same cloud-dependency problem as A4. As an *offline heuristic* it would be unreliable theater. Low priority; revisit only if a specific user need emerges.

### A6. Citations — **M** ✅ feasible and fits well

Generate APA/MLA/Chicago citations from a URL, DOI, or title:
- **Metadata lookup:** Crossref/DOI APIs are free, no key — one new Rust command (`fetch_citation_metadata`) using the existing `reqwest` stack. No capability changes (HTTP runs Rust-side; `capabilities/default.json` untouched).
- **Formatting:** the main cost. Either prompt the LLM to format (easy, occasionally wrong — citations are the one place users check every character) or vendor CSL/citeproc-style formatting in Rust (correct, ~a week). Middle path: fetch structured metadata from Crossref, format the 3 styles with template code, fall back to AI.
- **Delivery:** a `{cite}` macro in the `Macro` enum (touch `evaluate_macro` + `Db::expand`) plus a small input popup mode. Prompt-once input forms already exist (`aiarg` mode), so the interaction is solved.

### A-scope summary

| Feature | Effort | Verdict |
|---|---|---|
| Paraphrase modes | S–M | Build now — template system already fits |
| Grammar: on-demand | M | Build now — it's an AI action + polish |
| Grammar: while-typing | L | Skip / opt-in only — privacy + latency |
| Summarizer polish | S | Build now |
| Plagiarism | L | Defer — needs cloud search, anti-local-first |
| AI detection | M–L | Defer — weak value, unreliable |
| Citations | M | Build — Crossref + `{cite}` macro |

Sequencing: A1 + A3 in one release (both are mostly AI-action data + popup polish), then A6, then A2-on-demand.

---

## Part B — UI/UX improvement scope

### B1. Refactor the giant files (do this *before* adding AI UI) — **M**

The AI features above all land in the same hot files:
- `PhraseEditor.svelte` — **743 lines**, the single worst frontend file; split into field sections (triggers, macros, AI) as child components.
- `Popup.svelte` — **654 lines** orchestrating a 7-mode union (`search|inputs|choose|offer|ai|aiarg|airesult`); a citation-input mode would be #8. Extract each mode into a component + a typed mode-state module before it calcifies.
- Rust: `autotext.rs` (**3,555 lines** — keystroke listener, SymSpell engine, offer flow, hotkey parsing in one file), `commands.rs` (2,056), `repo.rs` (2,617). Split `autotext.rs` into `listen` / `spellcheck` / `offer` modules.

### B2. Accessibility — **S–M**

Real gaps:
- No `aria-live` on streaming AI output or toasts — screen readers miss results entirely.
- `common/Modal.svelte` has no focus trap; `PhraseList`/`FolderTree` rows are `<div>`s with click handlers instead of buttons/listbox roles.
- Popup navigation is routed from the Rust listener as `offer-key` events — invisible to the DOM a11y tree; add parallel DOM keydown handling where the popup has focus.

### B3. Consolidate repeated frontend patterns — **S**

Three patterns re-implemented per component: skeleton+`$effect` data loading, settings draft-copy/save, `errMsg`+toast error handling. Extract `createResource`, a `useDraft` helper, and a shared error handler. Pays for itself on every new panel (A1/A3/A6 each add one).

### B4. Theme parity + polish — **S**

`app.css` design tokens are solid, but the **light and sepia theme blocks are less complete than dark** (e.g. logo overrides only handled for dark). Audit all tokens per theme; fix the retired Groq model in `AiPage.svelte` presets (`mixtral-8x7b-32768` was decommissioned) while in there.

### B5. Popup UX refinements — **S–M**

- Spellcheck-offer window sizing is hardcoded (width 280–380, height formulas) — move to the content-adaptive ResizeObserver pattern the other popup modes already use.
- The 120ms hide-before-inject settle delay is a latent race with aggressive-focus apps; worth a rework if users still report misdirected input.

### B6. Larger bets (only if the audience grows) — **L each**

- **i18n:** completely absent — every string inline in components *and* Rust error messages. Svelte-side can adopt `svelte-i18n` + message catalogs incrementally; Rust messages need a mapping layer. Only worth it with non-English users.
- **Frontend test coverage:** vitest exists but there's exactly one test file and no `vitest.config`. The popup mode machine and editor logic deserve tests before they get refactored (B1) — write characterization tests first, then split.

### B-scope summary

| Item | Effort | When |
|---|---|---|
| Split PhraseEditor + Popup + autotext.rs | M | Before A1/A3/A6 UI work |
| Accessibility (live regions, focus trap, list semantics) | S–M | With popup refactor |
| Pattern consolidation (load/draft/error) | S | Before adding new panels |
| Theme parity + stale presets | S | Any release |
| Popup sizing/offer rework | S–M | With popup refactor |
| i18n | L | On demand |
| Frontend characterization tests | M | Before refactors |

**Suggested order:** characterization tests → Popup/Editor refactor → A1+A3 (paraphrase modes + summarizer) riding the cleaned-up popup → a11y pass on the new popup modes → A6 citations → A2 on-demand grammar.
