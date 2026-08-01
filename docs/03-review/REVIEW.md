# REVIEW.md — Spec Gate (P3)

> **SDD Stage:** P3 — Review (gatekeeper)
> **Role:** Senior Spec Reviewer
> **Input:** `docs/02-spec/SPEC.md` (cross-checked against `docs/01-requirements/REQUIREMENTS.md`)
> **Checks:** Completeness · consistency with requirements (FR/NFR IDs) · feasibility · security.

---

## VERDICT: ✅ PASS (with 3 non-blocking observations, resolved below)

The spec is complete, internally consistent, feasible, and secure for the v1 single-user-local
scope. Every FR/NFR in `REQUIREMENTS.md` has a corresponding schema, contract, or error code.
Three spec-completeness gaps were found during review; none block, and each has been given a
binding v1 resolution (folded into the spec) so the implementation is unambiguous.

---

## 1. Completeness ✅

| Requirement | Spec coverage | Status |
|---|---|---|
| FR-1 Phrase CRUD | §3.4 Phrase, §6 rows 6/8/9/10 | ✅ |
| FR-2 Folder CRUD + acyclic | §6 rows 2–5, `cycle` error §8 | ✅ |
| FR-3 Triggers (autotext/hotkey) | §3.3 TriggerBinding, `/expand` resolve | ✅ |
| FR-4 Execution pipeline | §6 row 11, §7 expand_phrase | ✅ |
| FR-5 Macro eval (datetime/datemath/inputform) | §3.1, §7, `missing_input` | ✅ |
| FR-6 Abbreviation uniqueness | `conflict→409` §6/§8 | ✅ |
| FR-7 Soft-delete | §6 rows 7/10, `include_deleted` | ✅ |
| FR-8 Sensitive masking | §3.6 PhraseSummary, §9 | ✅ |
| FR-9 Clipboard bounded history | §6 rows 12/13 — **see O1** | ✅ (resolved) |
| FR-10 Ordered segments | §3.2, §7 | ✅ |
| FR-11 Local persistence | §4 ORM, SQLite | ✅ |
| FR-12 Expansion inputs | §3.5 ExpandRequest | ✅ |
| NFR-1..10 | §1, §3 (v2 unions), §5 (port), §6.1, §8, §9 | ✅ |

---

## 2. Consistency with Requirements ✅
- All `code` strings in §8 appear in the API table (§6); no orphan codes.
- Status codes match: `404` for missing/deleted, `409` for conflicts, `422` for validation/reserved
  type, `400` for missing input — consistent with NFR-4/NFR-5.
- Discriminated-union design (§3.1/§3.2) directly satisfies NFR-1; the JSON-column persistence
  (§4) preserves that without flattening the model.

---

## 3. Feasibility ✅
- Single dependency addition (`sqlalchemy`) via `uv`; SQLite is stdlib-backed (no driver needed).
- All endpoints are standard FastAPI + sync SQLAlchemy patterns; no exotic infra.
- `expand_phrase`/`evaluate_macro` are pure and unit-testable (NFR-3) independent of the DB.

---

## 4. Security ✅
- Single-user, local file, no network egress (NFR-10) — minimal attack surface.
- Sensitive masking enforced at the summary boundary (FR-8); bodies only on direct GET/delivery.
- No secrets/credentials in code; logging must exclude bodies (§9). No injection vectors
  (no raw SQL; ORM + Pydantic validation at the boundary).

---

## 5. Observations (non-blocking) — RESOLVED

### O1 — Clipboard capacity not pinned (FR-9)
**Finding:** FR-9 acceptance requires capacity-bounded eviction, but SPEC did not name the constant.
**Resolution (binding for impl):** `CLIPBOARD_CAPACITY = 100`; on overflow evict oldest by
`captured_at`. *(Spec §1/§4 amended.)*

### O2 — Hotkey uniqueness undefined ⚠️ minor ambiguity
**Finding:** FR-6 constrains only `abbreviation`; two phrases sharing a hotkey is unspecified.
`resolve()` would return the first match silently.
**Resolution (binding for impl):** v1 **does not** enforce hotkey uniqueness; `resolve()` returns the
first match in storage order. This is deterministic and acceptable for single-user v1; flagged for a
future menu/selection feature (PhraseExpress parity item, already in the Deferred Register).
*(Spec §10 Q3 added.)*

### O3 — Folder deletion leaves orphaned phrases ⚠️ material correctness gap
**Finding:** Deleting a folder that still contains (non-deleted) phrases was unspecified. With
SQLite FK enforcement off by default this orphans rows; with it on it raises an unhandled
`IntegrityError` → 500.
**Resolution (binding for impl):** v1 **restricts** folder deletion — deleting a folder that contains
any non-deleted phrase returns `409` with code `folder_not_empty`. This is safe and explicit.
*(Spec §6 row 5 and §8 error catalog amended.)*

> Because O1–O3 are non-blocking and now have binding resolutions, the verdict is PASS. The
> implementation in P4 must honor O1–O3 exactly; tests in P5 must cover them (capacity eviction,
> resolve-first hotkey, and `folder_not_empty` 409).

---

## 6. Outcome
Pipeline may proceed to **P4 (code)**. Required P5 test additions: capacity-eviction (FR-9/O1),
`folder_not_empty` 409 (O3), and masked-summary (FR-8).
