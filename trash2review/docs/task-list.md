# PhraseXp Feature Integration — Task List

Derived from `docs/preliminary-report.md` (2026-10-27). Ordered tier-wise by
impact × fit. Each task lists acceptance criteria and primary touch points.
Check off items as they land; use `- [x]`.

Legend: 🔴 blocking pain · 🟠 table-stakes gap · 🟢 differentiator

---

## Tier 1 — High impact, natural fit

### 1.1 Paraphrase mode presets as builtin AI actions 🟢 *(from QuillBot)* — **done 2026-10-28**
- [x] Add builtin actions: Fluency (`ai-fluency`), Academic (`ai-academic`); Formal/Casual/Concise/Expand already existed
- [x] Each preset = prompt template over `{selection}`; editable inline like existing actions
- [x] New builtins appended to `BUILTIN_AI_ACTIONS` (18 total); `INSERT OR IGNORE` seeding verified by existing tests
- Touch: `domain/models.rs`, `AiActionsEditor.svelte`

### 1.2 Tone detection & tone rewrite 🟢 *(from Grammarly)* — **done 2026-10-28**
- [x] "Detect tone" action (`ai-tone-detect`) returning compact tone labels
- [x] Tone-targeted rewrite actions: Friendlier (`ai-friendlier`), More direct (`ai-direct`); formal/casual/confident already existed
- [x] Tone context: global `ai_instructions` brand voice already rides as the system message on every call (existing architecture; no change needed)
- Touch: `domain/models.rs`, `triggers/ai.rs`

### 1.3 Summarizer action with length controls 🟢 *(from QuillBot)* — **done 2026-10-28**
- [x] Summarize retemplated to `Summarize — target length {length} (e.g. 30 words, 3 bullets)` via `RETEMPLATED_AI_ACTIONS` (untouched rows only)
- [x] Generic `{token}` param plumbing: Rust `param_token`/`required_param`/`substitute_template` + frontend regex mirror; `PopupAiArg.svelte` generalized with per-token prompt copy (language/length/custom)
- Touch: `commands.rs`, `PopupAiArg.svelte`, `Popup.svelte`

### 1.4 Streaming AI results in the popup 🟢 — **done 2026-10-28**
- [x] `triggers/ai.rs::complete_stream` — SSE `chat/completions` streaming (reqwest `stream` + futures-util), deltas → `ai-delta` events (accumulated text)
- [x] `PopupAiResult.svelte` renders progressively; Replace/Insert/Copy/Retry unlock on `ai-result`; header shows "Generating…"
- [x] `ai_cancel` command + `AppState.ai_cancel` flag; mid-stream Cancel/Esc aborts, restores clipboard, emits `ai-cancelled`; retry streams too
- Touch: `triggers/ai.rs`, `commands.rs`, `lib.rs`, `invoke.ts`, `Popup.svelte`, `PopupAiResult.svelte`

### 1.5 File-based sync 🟠 *(from TextExpander, local-first)* — **done 2026-10-28**
- [x] Design: one `phrasexp-library.json` snapshot in a user-chosen directory (Syncthing/WebDAV/git-friendly; temp-file + rename writes, mutex-serialized runs)
- [x] Export-on-change (create/update/delete fire best-effort sync) + import-on-detect (startup sync + "Sync now"), per-phrase LWW on `updated_at` (`sync.rs::merge_lww`, unit-tested)
- [x] Conflict safety net: `upsert_synced_phrase` writes timestamps verbatim (no ping-pong) and snapshots the previous row before overwrite; deletions propagate as tombstones
- [x] Exclusions: settings/counters/clipboard history never enter the snapshot
- Touch: new `sync.rs`, `db/repo.rs` (`upsert_synced_phrase`/`upsert_synced_folder`, `sync_dir` column), `commands.rs` (`sync_now`/`sync_set_dir`/`sync_status`), `lib.rs`, `DataPanel.svelte`, `types.ts`, `invoke.ts`
- Note: convergence verified by unit tests of the merge logic; a real two-machine soak test is still worth running.

---

## Tier 2 — High impact, more work

### 2.1 Fill-in form improvements 🟠 *(from PhraseExpress)*
- [ ] Date-picker input type
- [ ] Dropdown type populated from file contents (CSV/lines)
- [ ] Per-field default values
- [ ] Form memory: "reuse last values" checkbox per form
- Touch: `models.rs` (`InputForm` macro), `InputForm.svelte`, `repo.rs` storage of last values
- Done when: a form phrase can prefill from the previous run.

### 2.2 Macro recorder — Phase 1: keystrokes 🟢 *(from PhraseExpress)*
- [ ] Record mode capturing key sequence + inter-key delays into delay/key `OutChunk` chunks
- [ ] Editor UI to trim/re-record recorded macros
- Touch: new `recorder` module (reuse `rdev`/evdev listener), `PhraseEditor.svelte`
- Done when: a recorded 5-step keystroke sequence replays faithfully on X11.

### 2.3 Expansion statistics dashboard 🟠 *(from TextExpander)*
- [ ] Persist weekly expansion counts (new table or aggregate from `expanded_count`/`last_used_at`)
- [ ] Settings/Library view: weekly trend, top phrases, per-folder breakdown, "time saved" estimate (chars ÷ avg typing speed)
- Touch: `repo.rs` queries, new stats panel component
- Done when: dashboard shows real numbers from existing usage data.

### 2.4 Inline autocomplete suggestions 🟢 *(from PhraseExpress AutoComplete / Grammarly)*
- [ ] Ghost-text or minimal popup suggestion while typing (prefix match on active abbreviations)
- [ ] Tab/Enter to accept; per-app exclusion honored; off by default
- Touch: `triggers/autotext.rs` buffer matching, new lightweight overlay window
- Done when: suggestion appears in a plain GTK text field on X11 without stealing focus.

### 2.5 Rich-text authoring & clipboard-safe delivery 🟠 *(from PhraseExpress)*
- [ ] Expose HTML content type in the editor (currently latent in `ContentType`)
- [ ] Markdown/HTML delivery: save clipboard → paste rich → restore clipboard (via `arboard`) — fixes clipboard clobber
- [ ] Fix approximate cursor restoration on the paste path (`cursor` macro)
- Touch: `inject.rs`, `repo.rs` rich-text parsing, `PhraseEditor.svelte`
- Done when: expanding a Markdown phrase leaves the clipboard unchanged.

---

## Tier 3 — Strategic / differentiating

### 3.1 Offline grammar & style engine 🟢 *(from Grammarly, reinterpreted local-first)*
- [ ] Phase A (rule-based): repeated words, double spaces, capitalization, common confusions (their/there, its/it's) — extend SymSpell/Damerau infrastructure
- [ ] Phase B (model-based): optional on-device grammar rewrite action (local model via existing BYO endpoint)
- [ ] Delivery through the existing spellcheck offer/insert flow with its own aggressiveness setting
- Touch: `spellcheck` module, new `grammar` module
- Done when: Phase A catches seeded grammar errors offline in a test app.

### 3.2 Shareable snippet packs 🟠 *(from TextExpander teams, serverless)*
- [ ] Export folder → versioned, optionally signed pack file
- [ ] Import/update flow: diff against existing, prompt before overwrite, keep version-history snapshot
- Touch: new `packs` module reusing importer infrastructure in `commands.rs`
- Done when: a pack round-trips between two instances with update-diff behavior.

### 3.3 PhraseExpress-style macro functions
- [ ] Phrase nesting links (`%snippet%` equivalent — beyond current `nested` macro if needed)
- [ ] Conditional logic macro (simple if/else on input or clipboard content)
- [ ] External data source macro (read row/field from CSV)
- Touch: `models.rs` macro enum, expansion engine
- Done when: a phrase conditionally inserts based on clipboard content in a unit + manual test.

### 3.4 Translation action 🟢 *(from QuillBot)*
- [ ] Builtin action with `{language}` target param (reuse existing `aiarg` language flow)
- Done when: select → hotkey → translate to chosen language → Replace works.

---

## Hygiene & Platform Fixes (from report Weaknesses — do alongside tiers)

- [ ] 🔴 **W1:** Non-US keyboard layout support (layout-aware injection mapping) — `inject.rs`
- [ ] 🔴 **W2:** Wayland: document + smooth the `input` group / ydotool setup (guided setup screen); investigate portal-based global shortcuts — `evdev_listen.rs`, `lib.rs`
- [ ] 🟠 **W13:** Frontend tests: component tests for Popup modes and PhraseEditor; E2E smoke test (Playwright/WebDriver)
- [ ] 🟠 **W14:** Single source of truth for schema — generate/migrate from one definition, drop inline SQL duplication in `repo.rs`
- [ ] 🟠 **W15:** Update `HOW_TO_USE.md` to list all 18 macro kinds (uuid, shell, counter, image) and AI flow
- [ ] 🟢 **W16:** *(deferred — covered by 2.2 macro recorder)*
- [ ] 🟠 **W8:** *(covered by 2.3 stats)*

---

## Suggested Execution Order (quick wins first)

1. 1.1 Paraphrase presets → 1.2 Tone → 1.3 Summarizer → 3.4 Translation (all prompt-level)
2. 1.4 Streaming results
3. 2.3 Stats dashboard
4. 1.5 File-based sync
5. 2.5 Rich text + clipboard-safe delivery (with W1 layout work in parallel)
6. Tier 2 remainder, then Tier 3
