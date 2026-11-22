# Phase 1 Purge Plan — dry run

**Scan date:** 2026-11-21 · method: content-hash dedup (all 178 tracked files, zero collisions),
inventory `none`-row cross-check with per-file grep, Rust `mod` wiring trace
(`lib.rs` → `domain/db/triggers`), TS/Svelte import check, config-reference check
(tauri.conf.json, package.json, CI, capabilities/, Cargo.toml).

## Result: zero new moves. The live tree is already clean.

| path | reason | confidence | inbound refs | action |
|---|---|---|---|---|
| trash2review/docs/preliminary-report.md | Superseded one-off analysis (v1.4.x-era); content lives on in docs/feature-comparison.md. Already parked by earlier commit `0bb4d85`. | high | none | leave parked — human finalizes |
| trash2review/docs/task-list.md | Old generation of docs/task-list.md (diverged plan). Already parked by `0bb4d85`. | high | historical mention in docs/backlog-resolution-plan.md only | leave parked — human finalizes |
| trash2review/README.md | Holding-pen explainer; load-bearing while the pen exists | low | none | keep |
| .pipeline/* | Pipeline scratch artifacts, zero refs | medium | none | keep for pipeline duration |

## Explicitly checked and NOT flagged (false-negative risks ruled out)

- All inventory zero-ref rows are false negatives of the basename grep — verified imported/wired:
  `dates.ts`, `hdrag.ts`, `macros.ts`, `theme.ts`, `toastError.ts`, `fuzzy.ts`, `aiOptions.ts`,
  `diff.ts`, 8× `stores/*.svelte.ts`, `spellcheck.rs`, `evdev_listen.rs`, `error.rs`, `db/mod.rs`,
  all `android/` files, both dictionary `.txt` files (`include_str!` in `spellcheck.rs:192,198`).
- No `*_old`/`.bak`/`copy`/backup filenames; no committed build/cache artifacts; lockfiles intentional.
- `docs/feature-comparison.md` — zero refs but committed 2026-10-30 (<30 days) and load-bearing positioning doc.
- `Square*Logo.png`, `StoreLogo.png`, `64x64.png` — not in `tauri.conf.json` bundle.icon but standard
  `tauri icon` output for Windows bundling; moving risks breaking that target.

## Carry-over finding (logged for Phase 2/3)

- Stale comment: `desktop/src/lib/components/macros/MacrosPage.svelte:3` references
  `src-tauri/src/domain/macros.rs`, which does not exist (resolution lives in `domain/expand.rs`).

## Notes for the human

- `trash2review/docs/` still awaits your review/finalization — the pipeline does not empty it.
- The earlier pass (`0bb4d85`) did the real purge work; this phase confirms nothing new qualifies.
