# Plan: Resolve the Deliberately-Deferred Backlog

Triage of `docs/task-list.md` § "Backlog — deferred deliberately" (2026-10-30).
Grounded in current code; file:line refs verified today. Three items turn out
to be already shipped — the backlog is staler than it looks.

## Dispositions

| # | Item | Disposition | Why |
|---|---|---|---|
| 1 | While-typing sentence grammar | **Close as shipped** | Phase 5 already delivered exactly what A2 prescribed: `review_sentence` (`src-tauri/src/triggers/autotext/listen.rs:394`) fires after sentence end during typing, off by default, refuses to run when AI is disabled (`listen.rs:271-275`), spellcheck app-exclusions apply (`FeaturesPanel.svelte:57`). |
| 2 | Plagiarism detection | **Close as won't-do** | Scope doc A4: needs a cloud search index; every check sends user text to a third party — contradicts the local-first promise (HOW_TO_USE.md §17). No credible local version exists. |
| 3 | AI-content detection | **Close as won't-do** | Scope doc A5: detector accuracy is contested; an offline heuristic would be unreliable theater. Revisit only on a concrete user request. |
| 4 | i18n (B6) | **Keep deferred, with trigger** | Real cost (every Svelte string inline + Rust error messages need a mapping layer). Trigger to reopen: first substantive non-English user demand. Approach is pre-planned below so reopening is cheap. |
| 5 | Retired Groq preset | **Close as shipped** | `AiPage.svelte:14` model list is current (`llama-3.3-70b-versatile`, `llama-3.1-8b-instant`, `gemma2-9b-it`); no `mixtral-8x7b-32768` anywhere. |
| 6 | Offer-popup hardcoded sizing (B5) | **Do — the one actionable item** | `offer.rs:214-216` computes width/height from proxies (longest word, count) with hardcoded clamps 280–380 × 140–240. Long suggestion text wraps and clips. Effort S. |

## Execution steps

### Step 1 — Update `docs/task-list.md` backlog section
- Check off items 1 and 5 with one-line evidence (as done in earlier phases).
- Move items 2 and 3 to a new "Non-goals (closed)" subsection with the
  one-line rationale each — so they stop reading as "pending decisions".
- Item 4 stays in Backlog, annotated with its reopen trigger.
- Item 6 becomes the section's only open item, pointing at Step 2.

### Step 2 — Offer-popup content-adaptive sizing (item 6)
The scope doc's ResizeObserver suggestion assumed webview-side sizing; the
actual code resizes the native window Rust-side *before* show, from content
proxies. Two options:

- **Option A (recommended, S):** keep Rust-side sizing but measure the real
  rendered text instead of proxies — evaluate a `measure()` in the offer
  webview (or reuse the content-adaptive pattern `PopupSearch`/other modes use
  via `ResizeObserver` → `set_size` IPC) and resize once from the DOM.
  Deletes the `280/380/7.0/108/30/140/240` magic numbers at `offer.rs:214-216`.
- **Option B (fallback):** keep the formula, only fix the clamp bounds so
  long suggestions wrap instead of clip. Zero risk, cosmetic fix only.

Acceptance: offer popup shows 5+ suggestions with long words without
clipping; no regression in the 120ms hide-before-inject sequencing;
`cargo test` green (offer module has coverage in the listen/offer tests).

### Step 3 — i18n reopen-runbook (item 4, doc-only)
Append to the backlog item so future work doesn't re-derive it:
frontend `svelte-i18n` + `locales/{en,…}.json` catalogs, migrated
incrementally per component; Rust-side error strings get a
`code → message` mapping keyed by error enum variant (errors already flow
through IPC as structured values, so the frontend can own translation).

## Order and effort
Steps 1 and 3 are doc-only (one commit). Step 2 Option A is the only code
change, effort S (≤ 2 days incl. tests). Total: one release's worth of
backlog hygiene, after which the "Backlog" section contains exactly one
deferred item (i18n) with an explicit trigger.

## Non-goals of this plan
No behavior change beyond Step 2. Plagiarism and AI-detection stay out of the
product; this plan records that decision rather than revisiting it.
