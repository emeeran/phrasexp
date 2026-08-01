# REQUIREMENTS.md — phrasexp

> **SDD Stage:** P1 — Requirements
> **Role:** Senior Business Analyst
> **Inputs:** `docs/00-domain/DOMAIN.md`, `docs/00-domain/CONTEXT_MAP.md`
> **Rule:** Every requirement is **testable** and **traceable** to its originating sub-domain/context.

---

## 1. Scope Decisions (resolves `DOMAIN.md §9` gaps)

These product decisions were taken to scope the **v1 (MVP)** build. They are explicit, overridable
at the Review gate, and every deferred item is preserved in §5 (Deferred Register) — nothing in the
domain model is discarded.

| Gap (§9) | Decision for v1 |
|---|---|
| ⚠️1 Platform | **Windows-first** design target. The backend is an HTTP service; OS-level keystroke injection (C15) is **deferred** — delivery returns expanded text over the wire in v1. |
| ⚠️2 Licensing | **Free**, single-user. No editions, no entitlement gating. |
| ⚠️3 Cloud vs local | **Local-first / offline.** No network egress required. Sync/Team/Identity/AI are **deferred**. |
| ⚠️4 Persistence | Local **SQLite** database (single file). Data survives restarts. |
| ⚠️5 Privacy/compliance | Single-user local store; **NFR-9** forbids secrets in logs/responses. No multi-tenant data. |
| ⚠️6 Sync conflict | n/a — sync deferred. |
| ⚠️7 Macro power | **Constrained, terminating** macro set: `datetime`, `datemath`, `inputform`. No loops/recursion/external I/O in v1. |
| ⚠️8 i18n | **English UI strings**, UTF-8 throughout (NFR-8). |
| ⚠️9 Identity | Local single-user; no accounts/auth in v1. |
| ⚠️10 AI | **Deferred.** No external LLM calls. |

**v1 MVP slice** (from `DOMAIN.md §8`): Phrase Library (C2) + Trigger System autotext/hotkey (C4) +
Execution Pipeline (C1) + constrained Macro Engine (C3) + Clipboard Management (C7) + local
persistence + Security masking (C11).

---

## 2. Functional Requirements (FR)

### FR-1 — Phrase CRUD
A user can create, read, update, and delete a Phrase. Each Phrase has exactly one canonical body.
- **Trace:** C2 Phrase Library · `DOMAIN.md §5.1`
- **Acceptance (Given/When/Then):**
  - *Given* a folder exists, *When* I POST a valid phrase to that folder, *Then* it is stored and returned with an `id` and `201`.
  - *Given* a phrase exists, *When* I GET it by id, *Then* the full phrase (including body segments) is returned.
  - *Given* a phrase exists, *When* I PUT updated fields, *Then* only the supplied fields change and `updated_at` advances.

### FR-2 — Folder CRUD with acyclic tree
Folders organize Phrases in a hierarchy. A folder's `parent_id` must not create a cycle.
- **Trace:** C2 · `DOMAIN.md §5.2`
- **Acceptance:**
  - *Given* no folders, *When* I POST a root folder (no parent), *Then* it is created.
  - *Given* folder A→B→A would form a cycle, *When* I set the parent, *Then* the request is rejected (cycle error).

### FR-3 — Trigger bindings (autotext + hotkey)
A Phrase may carry trigger bindings: an **autotext** abbreviation and/or a **hotkey**.
- **Trace:** C4 Trigger System · `DOMAIN.md §5.5`
- **Acceptance:**
  - *Given* a phrase with an autotext abbreviation `br,`, *When* I expand by that abbreviation, *Then* the phrase resolves.
  - *Given* a hotkey trigger `ctrl+shift+b`, *When* I expand by that key, *Then* the phrase resolves.

### FR-4 — Execution pipeline (resolve → expand → deliver)
Expansion resolves a trigger to a phrase, evaluates its body, and returns the output.
- **Trace:** C1 Execution Pipeline · `DOMAIN.md §6`
- **Acceptance:**
  - *Given* a phrase matching the supplied trigger exists, *When* I POST `/expand`, *Then* the response contains the fully expanded output text.
  - *Given* no phrase matches, *When* I POST `/expand`, *Then* a `404 not_found` is returned.

### FR-5 — Macro evaluation (datetime, datemath, inputform)
Macros embedded in a body are evaluated at expansion time. Evaluation is pure/deterministic.
- **Trace:** C3 Macro Engine · `DOMAIN.md §5.3`
- **Acceptance:**
  - *Given* a `datetime` macro with format `%Y-%m-%d`, *When* expanded at a fixed `now`, *Then* the formatted date appears in the output.
  - *Given* a `datemath` macro with `days_offset=1`, *When* expanded, *Then* tomorrow's date appears.
  - *Given* an `inputform` macro for field `name`, *When* expanded with `inputs={"name":"Sam"}`, *Then* `Sam` appears in the output; without it, a `400 missing_input` error is returned.

### FR-6 — Abbreviation uniqueness
An abbreviation must be unique across non-deleted phrases. Conflict → `409`.
- **Trace:** C2/C1 invariant · `DOMAIN.md §5.1`
- **Acceptance:**
  - *Given* phrase X uses abbreviation `sig`, *When* I create/update another active phrase with `sig`, *Then* a `409 conflict` is returned and the change is rejected.

### FR-7 — Soft-delete (recycle bin)
Deleting a phrase soft-deletes it (`deleted_at` set); deleted phrases never resolve and are excluded from the default list.
- **Trace:** C2 · `DOMAIN.md §5.2`
- **Acceptance:**
  - *Given* a phrase exists, *When* I DELETE it, *Then* a `204` is returned and it no longer appears in the default list nor resolves on expand.
  - *When* I list with `include_deleted=true`, *Then* it appears.

### FR-8 — Sensitive-phrase masking
Phrases flagged `sensitive=true` are masked (`***`) in non-delivery list/summary views; the full body is only returned on direct read or delivery.
- **Trace:** C11 Security · `DOMAIN.md §7.3`
- **Acceptance:**
  - *Given* a sensitive phrase, *When* I list phrases, *Then* its `content_preview` is `***`.

### FR-9 — Clipboard capture + bounded history
Capturing text appends a clipboard entry; history is capacity-bounded (newest retained).
- **Trace:** C7 Clipboard Manager · `DOMAIN.md §5.4`
- **Acceptance:**
  - *Given* the clipboard is at capacity, *When* a new entry is captured, *Then* the oldest is evicted and total count stays ≤ capacity.

### FR-10 — Phrase body as ordered segments
A phrase body is an ordered list of segments, each either literal `text` or a `macro`. Segments concatenate in order during expansion.
- **Trace:** C3/C2 · `DOMAIN.md §5.1, §5.3`
- **Acceptance:**
  - *Given* a body `[text "Hello ", inputform "name"]`, *When* expanded with `name=Sam`, *Then* output is `Hello Sam`.

### FR-11 — Local persistence
Phrases, folders, and clipboard entries persist across process restarts in a local SQLite database.
- **Trace:** Local-first decision (§1) · NFR-6
- **Acceptance:**
  - *Given* a phrase created in one process, *When* the process restarts and I GET the phrase, *Then* it is returned with its data intact.

### FR-12 — Expansion inputs
The expand endpoint accepts an `inputs` map supplying values for any `inputform` macros in the resolved phrase.
- **Trace:** C3 · `DOMAIN.md §5.3`
- **Acceptance:**
  - *Given* a phrase with inputform field `city`, *When* I POST `/expand` with `inputs={"city":"Berlin"}`, *Then* `Berlin` is substituted.

---

## 3. Non-Functional Requirements (NFR)

| ID | Requirement | Trace |
|---|---|---|
| **NFR-1** | Type safety via **Pydantic v2** discriminated unions; no untyped `dict` in the public contract. | p4_code |
| **NFR-2** | Standard FastAPI patterns; OpenAPI docs auto-served at `/docs`; storage injected via DI. | p4_code |
| **NFR-3** | Deterministic expansion: identical `(phrase, inputs, now)` ⇒ identical output. | §7 invariant 2 |
| **NFR-4** | Invalid request payloads are rejected with `422` before reaching domain logic. | — |
| **NFR-5** | Structured errors `{"code":..., "message":...}` with correct HTTP status codes. | — |
| **NFR-6** | Storage behind a **Repository port**; v1 impl is SQLAlchemy/SQLite, swappable without touching routes. | §1 ⚠️4 |
| **NFR-7** | Startup < 2s; `/expand` < 50ms p95 for a library ≤ 10k phrases. | — |
| **NFR-8** | UTF-8 end-to-end; English UI/error strings. | §1 ⚠️8 |
| **NFR-9** | No secrets in logs; sensitive values never echoed in non-delivery responses. | §7 invariant 3 |
| **NFR-10** | No required network egress; runs fully offline (no cloud/AI calls). | §1 ⚠️3/⚠️10 |

---

## 4. Traceability Matrix

| FR/NFR | Sub-domain / Context | `DOMAIN.md` ref |
|---|---|---|
| FR-1, FR-2, FR-6, FR-7 | C2 Phrase Library | §5.1, §5.2 |
| FR-3 | C4 Trigger System | §5.5 |
| FR-4, FR-10 | C1 Execution Pipeline | §6, §5.1 |
| FR-5, FR-12 | C3 Macro Engine | §5.3 |
| FR-8 | C11 Security | §7.3 |
| FR-9 | C7 Clipboard Manager | §5.4 |
| FR-11 | Local-first persistence | §1 ⚠️4 |
| NFR-1,2,4,5 | (cross-cutting API quality) | — |
| NFR-3 | Domain determinism | §7 |
| NFR-6 | Repository port | §5 (port) |
| NFR-7,8,9,10 | (operational) | §7 |

---

## 5. Deferred Register (out of v1, preserved for later)

Each maps to a sub-domain in `DOMAIN.md` and is **not lost** — merely postponed:
Cloud **Synchronization** (C8) · **Collaboration/Team** (C9) · **Identity & Access** (C10) ·
**AI Integration** (C14) · **Telemetry** (C13) · advanced scripting: loops/conditionals/external-data
(C3) · **Spelling Correction** (C6) · **Autocomplete/Prediction** (C5) · **Import/Export** (C12) ·
OS-level keystroke injection & multi-platform host port (C15) · formatted content delivery
(RichText/HTML/Word — stored type reserved, rendering deferred).
