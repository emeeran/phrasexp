# Phase 3 Style Plan — dry run (2026-11-21)

Convention detection: no rustfmt.toml / .prettierrc / eslint config exists. The Rust
side de-facto standard is default `rustfmt` (identifiers, doc-comment prose style, and
module layout are already consistent with it — the code just isn't fmt-clean). The
frontend has NO formatter configured: prettier/eslint are not dependencies of this
project, so imposing one would be a new convention, not a detected one. Frontend style
consistency was instead verified by inspection in the Phase 2 scans (naming, import
grouping, error-handling via errMsg/errToast, no console.logs, no emoji) — no
inconsistencies found.

## Inconsistencies found

| # | Dimension | Files | Winner convention | Action |
|---|---|---|---|---|
| 1 | Rust formatting | 77 hunks across ~20 files (75 pre-date this pipeline; repo was never fmt-clean) | default rustfmt | `cargo fmt` — apply |
| 2 | Error-handling philosophy | Rust: DomainResult + ApiError everywhere (single taxonomy, `db_err` misuse already flagged in Phase 2 plan → AUDIT.md); frontend: errMsg/errToast everywhere | existing | nothing to change |
| 3 | AI voice / emoji in user-facing text | none found (0 emoji, 0 "Success!", 0 console.log, 1 TODO total, in user-facing AiPage copy) | n/a | nothing to change |
| 4 | Naming | no Manager/Helper/Util suffixes; renames done in Phase 2 (parse_chord, snapshot_timestamp, disableNested) | existing | nothing to change |
| 5 | Import/module hygiene | consistent grouping observed (std → external → crate/super); rustfmt does not reorder imports | existing | nothing to change |
| 6 | Test padding | all 168 Rust + 42 frontend tests assert real behavior (Phase 2 scans); no getter-roundtrip or auto-generated test names | n/a | nothing to change |

## Apply step

`cargo fmt` + full gates (cargo test, clippy, vitest, svelte-check). One commit:
`phase(3): style and authorship consistency pass`.

## Deliberately not done

- Adding prettier/eslint: not this project's convention; would impose a new toolchain.
- Import reordering beyond rustfmt: no inconsistency to fix.
