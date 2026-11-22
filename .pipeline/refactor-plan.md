# Phase 2 Refactor Plan — dry run (2026-11-21)

Scans: Rust core (13,688 lines) + frontend (8,909 lines incl. tests/css). All items below are
**no observable behavior change** unless listed under "Flagged — not auto-fixing".

## Apply units (safe)

### R1 — db/repo.rs dedup (~85 lines)
- Extract `row_exists(conn, sql, id)` — the exists-probe block is copy-pasted 8×
  (repo.rs:271,323,334,361×2,383,420,623).
- Extract `encode_phrase_row(crypto, phrase)` — serialize+encrypt triple re-derived in
  `create_phrase` / `upsert_synced_phrase` / `update_phrase` (repo.rs:436,513,640).
- Fold `expand_full` into `expand` (repo.rs:965-1008) — 4-deep wrapper chain where
  `expand_full` has one caller.
- Delete orphaned doc at repo.rs:480-481 (describes `get_active_phrase`, dangles over `get_phrase`).

### R2 — commands.rs dedup (~60 lines)
- Extract `after_library_change(state, app, sync)` — post-mutation tail block repeated 13×
  (lines 63,83,147,164,180,330,880,1061,1078,1139,1222,1446,1560; each site keeps its exact
  call set via parameters).
- Extract clear-offer block — 7 identical lines in `apply_correction` (1609) / `delete_offer_word` (1652).
- Merge `create_labeled_phrase`/`create_simple_phrase` import creators (1084 vs 1514) — one fn
  with `Option<&str>` label (verify `text.is_empty() → abbr` name case).
- Extract `is_param_token(token)` predicate (630 vs 685).
- Inline the `clamp` closure (1738); drop unreachable `unwrap_or("")` arms in `import_csv` (873-874).
- Fix wrong doc on `resize_popup` (1726-1732) — grab text belongs on `grab_keyboard` (1746, undocumented).

### R3 — lib.rs (~25 lines)
- Inline `resolve_db_path` (159-161, one caller); inline `Crypto::disabled()` (crypto.rs:79, one caller).
- Reuse first `load_settings()` binding in `reregister_hotkeys` (424 loads twice).
- Dedup `refresh_app_poll_needed` computation in setup (691-701 duplicates lib.rs:90-108) —
  construct atomics first, manage state, then call the method.
- Delete stale first doc line on `dispatch_app_token` (282-284).

### R4 — triggers/ dedup (~180 lines)
- Extract `summon_popup(app, size)` — mouse-coords/set_position/show/focus/macOS-show triplicated
  (offer.rs:195, offer.rs:300, lib.rs:294; per-site size/visibility branches stay at call sites).
- Extract `ydotool_key(codes)` — 5 identical `Command::new("ydotool").args(["key",…])` forks (inject.rs:148-235).
- Hoist `png_rgba`+`arboard::ImageData` construction shared by both `paste_image` twins (inject.rs:73 vs 188).
- Share `build_messages`/endpoint-gate/error-detail between `complete` and `complete_stream` (ai.rs:15 vs 86).
- Share longest-suffix scan helper in index.rs (127 vs 343).
- Extract `erase_len(token)` — split_token+backspace math in spellcheck.rs:284, offer.rs:284, spellcheck.rs:709
  (tests at offer.rs:352-392 pin counts).
- Extract `current_word(buffer)` — 4× duplicated trim/rfind/split_token (listen.rs:703,768; spellcheck.rs:588,609).
- Dedup modifier-list ×4 in listen.rs (26,520,812,822) via `is_modifier`/shared fn.
- Delete dead `_` arm in `offer_nav_key` (listen.rs:58-69; outer match already narrowed to Num1..Num9).
- Rename `parse_spellkey` → `parse_chord` (listen.rs:174 — it parses every hotkey chord, not just spellkey).
- Rename `sync.rs:94` `now_iso` → `snapshot_timestamp` (distinct format, LWW load-bearing; prevents
  future "unification" silently changing merge order).
- Split mixed doc on `apply_case_mode_chunks` (expand.rs:76-81); resolve `// ponytail:` markers
  (expand.rs:80, inject.rs:398).

### F1 — frontend dead surface (~60 lines)
- Inline `resource.svelte.ts` (55 lines) into `PhraseEditor.svelte` — single caller, only
  `.data`/`loadOnce()` ever used; delete dead `load/loading/loaded/error` surface.
- Un-export `Draft<T>` (draft.svelte.ts:7); un-export `TextSegment`/`MacroSegment` (types.ts:145).
- Delete unreachable overlay `Escape` keydown on Modal.svelte:63 (window capture handler always wins).
- Collapse `Toast.svelte` kindIcon 4-entry map → `t.kind === "ok" ? "check" : "info"`.
- Rename `disablenested` → `disableNested` (MacroPicker.svelte + PhraseEditor.svelte:376).
- Move `abbrEl` declaration above first use (QuickAdd.svelte:50/59).
- Fix stale comment MacrosPage.svelte:3 (references nonexistent `domain/macros.rs`; actual: `domain/expand.rs`).

### F2 — frontend dedup (~15 lines)
- Collapse the 3 duplicated release→hide→120ms-settle sequences in Popup.svelte (120,209,318) via
  the existing `settleThen` — byte-identical sequences.
- Extract `triggerOf(type, value)` in invoke.ts (58 vs 69).

**Estimated total: ~425-470 lines removed, 0 behavior change.**

## Flagged — NOT auto-fixing (goes to AUDIT.md)

Rust: `db_err` flattens all SQL errors to `Validation` (frontend-inspected `code` contract);
silent corrupt-JSON→empty phrase (repo.rs:1577); import DB-error swallowing (commands.rs:1096,1526);
`guard_no_cycle` self-declared defensive (repo.rs:1618); `maybe_expand`/`fire_delayed` ~100-line
duplication (concurrency-sensitive, comments document empirically-fixed races); 23-arg listener
`start()` → `ListenerCtx` struct (large diff); `AppState` god object (~40 lock sites);
`core/` workspace crate is P0-spike scaffolding (product decision — Android IME).
Frontend: PopupSearch/PopupChoose ~85-line near-verbatim duplication (CSS-scoping moves);
settings-panel card/toggle CSS ~80 lines promotable to app.css (cascade risk); Popup.svelte:266
unreachable guard (one token, kept).

## Test gates per unit
R1/R2/R3/R4: `cargo test` (171 tests) + `cargo clippy -- -D warnings`.
F1/F2: `npm run test` (42 tests) + `npm run check`.
Baseline: 171 Rust + 42 frontend, all green (.pipeline/baseline-test-results.txt).
