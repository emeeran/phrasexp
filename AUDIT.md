# Production Readiness Audit — 2026-11-21

Scope: PhraseXp 1.7.0 desktop app (Rust core `desktop/src-tauri/src/`, Svelte 5 frontend
`desktop/src/`). Six parallel audit dimensions; findings merged below. Line refs are
approximate to the commit at audit time (`9359b1a` + phase(3) fmt).

## Summary

**Ready with fixes.** The architecture, test discipline (168 Rust + 42 frontend, all four
CI gates blocking), crypto design (AES-256-GCM, Zeroizing DEK, keyring), SQL hygiene
(fully parameterized), Tauri capability scope (minimal), and frontend error surfacing are
solid. What blocks shipping: (1) the `Shell`/`File`/`Env` macros are reachable from
hostile input via JSON import and the sync snapshot file — arbitrary code execution;
(2) soft-delete doesn't bump `updated_at`, so sync tombstones never win and deletions
flip-flop; (3) injection failures are completely silent — the headline "I typed my
abbreviation and nothing happened" failure is untraceable; (4) a corrupt sync snapshot is
treated as empty and overwritten, silently destroying other devices' data. All four have
in-repo fix patterns; none are architectural.

## Blockers (must fix before prod)

All four blockers are FIXED as of 2026-11-21 on `cleanup/pipeline-2026-11-21`
(commits `b07bbe7`, `de6397c`, `6810981`, `8a42466`, `7fbb2bb`); checkboxes kept for the
record. Suite at fix time: 174 Rust + 42 frontend, clippy `-D warnings` clean.

- [x] **UNTRUSTED MACROS** `domain/expand.rs:79-92` + `commands.rs:1313` (`import_json`) +
  `sync.rs` — FIXED: `Phrase::has_untrusted_macros()` (models.rs) is enforced at both
  trust boundaries — `import_json` skips such phrases; `sync_now` refuses them before the
  merge (so they are neither applied nor propagated in the write-back snapshot), and
  invalid rows no longer abort the run mid-apply (`report.skipped`).
  *(Found independently by both the security and correctness auditors — high confidence.)*
- [x] **TOMBSTONE LWW** `db/repo.rs` (`soft_delete_phrase`, `merge_folders`) — FIXED: both
  now bump `updated_at`, so tombstones and folder moves win the LWW compare and propagate.
  Regression test: `soft_delete_and_folder_merge_bump_updated_at`.
  *(correctness auditor)*
- [x] **SILENT INJECTIONS** `triggers/inject.rs` + call sites — FIXED: `EnigoInjector`
  propagates enigo failures (`enigo_err`), `ydotool_key` checks the child's exit status,
  `deliver_chunks` logs failures unconditionally (and only records `LAST_DELIVERY` on
  success), and every best-effort step goes through `inject::log_if_err`.
  *(observability auditor)*
- [x] **CORRUPT SNAPSHOT WIPE** `sync.rs` — FIXED: missing file = first sync; read or
  parse failure = refuse with an error naming the file, leaving it byte-identical
  (tests: `corrupt_snapshot_refuses_sync_and_preserves_file`,
  `missing_snapshot_file_is_first_sync_not_corruption`,
  `hostile_snapshot_shell_macro_is_never_applied`).
  *(Found independently by both the operations and correctness auditors — high confidence.)*

## High priority

- [ ] No persistent error/log surface for a tray-hidden app. `debug_log` is stderr-only
  (discarded for GUI launches); the `phrase-error` event is only handled in the main
  window's webview, which starts hidden (lib.rs:643-645) — the codebase's own comments
  (commands.rs:489-497) acknowledge toasts are never seen. Expansion errors, keychain
  degradation (lib.rs:625), migration failures (lib.rs:637), and the "no key source at
  all" path (listen.rs:846) are all stderr-bound. A file log (or system notification) is
  the single highest-leverage observability fix. *(observability)*
- [ ] `db/repo.rs:598-614` — `update_phrase` SQL omits `folder_id`: moving a phrase is
  silently dropped while the returned `Phrase` (with the frontend-sent folder applied at
  commands.rs:1895) claims success. UI shows the move; the row doesn't have it. *(correctness)*
- [ ] `db/crypto.rs:85-92` — a corrupt keychain DEK silently regenerates: every `enc1:` row
  (sensitive bodies, clipboard history, AI key) becomes a locked placeholder forever, no
  warning. Refuse-and-surface instead of overwrite. *(Found by both ops and correctness —
  high confidence.)*
- [ ] `triggers/inject.rs:244-258` — clipboard restore is skipped when `set()` or `paste()`
  errors after the clobber: the user's clipboard is permanently replaced. Move restore to a
  path that runs on all outcomes once `set` succeeded. Related: only clipboard *text* is
  saved (images destroyed), and a sensitive phrase sits in clipboard-manager history
  (inject.rs:244-259, lib.rs:210-263). *(ops + security)*
- [ ] `triggers/autotext/listen.rs:582-614` — stale-correction undo: pressing Left arrow
  long after a silent correction backspaces over whatever text now sits at the caret. Add a
  recency/keystroke-staleness bound. *(correctness)*
- [ ] `triggers/autotext/expand.rs:393-422` + `listen.rs:478-486` — echo-suppression drops
  ALL key events while `inj_flag` is set, and *user* keystrokes refresh the echo timer, so
  typing during/after an expansion can be swallowed indefinitely. *(correctness)*
- [ ] Listener threads have no `catch_unwind` (~20 spawn sites); one panic in the evdev
  callback kills that device's reader permanently (the rescan skips `opened` nodes —
  `evdev_listen.rs:60-69`) while `autotext_status` keeps reporting `running: true`
  (listen.rs:849 only runs on normal return). *(observability + ops)*
- [ ] `commands.rs:1863-1869` — encrypt/decrypt-at-rest migration failure on the settings
  toggle is fully swallowed (`let _ =`), leaving a mixed plaintext/cipher library with a
  success toast. The startup path at least eprintlns; this one needs surfacing. *(Found by
  three auditors — observability, correctness, ops — high confidence.)*
- [ ] `sync.rs:126` — snapshot writes *decrypted* sensitive bodies as plaintext JSON to the
  (typically cloud-synced) folder, silently defeating at-rest encryption. `export_library`
  documents the risk (commands.rs:843-845); sync does not. *(security)*
- [ ] `commands.rs:55` (+ :754) — `let _ = rebuild_index(...)`: a failed rebuild leaves a
  stale abbreviation index after a "successful" save — phrases silently stop triggering.
  At minimum `debug_log` it. *(observability)*
- [ ] `commands.rs:1117-1230` — the AutoHotkey import parser has zero tests (all four other
  import formats have dedicated suites). *(testing)*
- [ ] `.github/workflows/release.yml` — the tag-driven release path runs no tests at all
  (tags don't trigger ci.yml); a tag cut on a broken main ships installers. *(testing)*

## Medium

- `domain/mod.rs:13` + `sync.rs:63` — LWW compares RFC-3339 strings with variable
  fractional precision; `.000` can beat a newer `.500` edit. Parse to `DateTime` instead.
- `sync.rs:134-139` — remote child folder upserted before its parent → FK violation aborts
  the entire sync permanently for that snapshot.
- `tauri.conf.json:36` — `csp: null`; no current injection sink exists (no
  `{@html}`/`innerHTML`), but both webviews render attacker-influenced strings and IPC
  exposes the decrypted AI key (repo.rs:1181-1189 → `get_settings`). Set a strict CSP;
  consider masking the key in `get_settings`.
- `commands.rs:217-224` — `read_image` path check doesn't canonicalize; `..`-style escapes
  let the webview read any `*.png` on disk. (save_image is clean; no size caps on
  `save_image`/import payloads either — memory DoS from a compromised webview.)
- `commands.rs:1405` — imported folder creation swallowed (`let _ =`); deferred folders can
  vanish while the import reports success.
- `db/repo.rs:189-244` — `run_migrations` ignores every `ALTER TABLE` error; a real failure
  (locked/disk-full) is indistinguishable from the expected duplicate-column.
- `db/repo.rs:277` (~60 sites) — `conn.lock().unwrap()` on the DB: one panic while holding
  the lock poisons it process-wide. Everywhere else tolerates poisoning
  (`into_inner()`); the DB should too.
- `lib.rs:792` — tray Quit is a bare `app.exit(0)`; no drain for in-flight injections or
  sync (mid-delivery quit = erased abbreviation, no expansion).
- `db/repo.rs:737-747` — `reset_database` doesn't tombstone or clear the sync snapshot
  (next sync resurrects everything), isn't crash-atomic, and leaves spellcheck/counters.
- `triggers/clipboard.rs:26,56,61` — clipboard capture DB-write failures silent; init
  failure kills history permanently with stderr only.
- `commands.rs:1742,1747` — `grab_keyboard` eprintlns bypass the debug_log facility (the
  success line is pure leftover noise).
- `desktop/package-lock.json:3` — root version stale at `1.3.1` (package.json says 1.7.0):
  lockfile not regenerated since the bump; re-run `npm install`.
- `src/lib/components/popup/Popup.svelte` + `PopupAi.svelte` — the AI-options UI layer has
  no component tests (parser is well tested; consumer isn't). Needs
  `@testing-library/svelte`.
- `sync.rs:48-78` tests — LWW tie (equal timestamps) and remote-tombstone-meets-tombstone
  behavior unasserted; `sync_now` roundtrip untested.
- `desktop/src/lib/aiArgs.test.ts:58-69` — duplicate `describe("argPromptFor")` block;
  delete it.
- `lib.rs:191-264` — selection-AI capture clears the clipboard and restores on only one
  error path.

## Low

- `update_phrase` returns pre-update `updated_at` (commands.rs:172-174).
- Trailing `{cursor}` chunk with no following text is ignored (inject.rs:334-353).
- Abbreviations > 64 chars can never fire (buffer cap, no validate() limit);
  `key_to_char` is US-layout only — unmapped keys clear the buffer silently (models.rs:533,
  listen.rs:23).
- Invalid regex abbreviations silently dropped at index build (index.rs:90), untested.
- evdev `opened` set never prunes dead nodes (suspend/resume renumbering).
- `spellcheck_learned` table + ignore list unbounded (user-paced; contrast
  MAX_PHRASE_VERSIONS=50). `clipboard_max_entries = 0` is an advertised unbounded history.
- `featuresPanel.svelte:14` — crypto-status IPC failure defaults to "available" (fail-open;
  should fail closed).
- DOI interpolated into Crossref URL unencoded (citation.rs:158; fixed host, no SSRF).
- `PHRASEXP_DEBUG=1` is undocumented in README/docs; when enabled it logs typed buffers
  (opt-in, but worth a scrub note in docs).
- No coverage measurement anywhere in CI; `core/` crate not linted by desktop CI.
- Portable-mode marker removal relocates the DB silently (lib.rs:143-155) — "everything
  disappeared" support scenario.

## Explicitly out of scope / accepted risk

- **Empirically tuned magic numbers** — `CLIPBOARD_PASTE_SETTLE_MS = 200`, enigo/ydotool
  cutoffs, 120ms popup settle, 500/60ms echo windows: documented as tuned; leave.
- **`db_err` SQL-error flattening** to `Validation` (repo.rs:20-22) — changes the
  frontend-visible error `code` contract; a `DomainError::Database` variant is the honest
  fix but is a behavior change. Flagged in the Phase 2 plan; deferred.
- **Silent corrupt-JSON → empty phrase** on decode (repo.rs:1515/1121) — deferred for the
  same reason (changes what damaged rows display).
- **Import DB-error swallow** (commands.rs create paths, "skip on duplicate" semantics) —
  surfacing changes import counts; deferred.
- **`maybe_expand`/`fire_delayed` ~100-line structural duplication** (expand.rs) — real
  duplication but interleaved with empirically-fixed echo-race ordering; any merge is a
  concurrency change, not a mechanical one.
- **23-arg listener `start()` / `AppState` god object** — large diffs, low value per line.
- **`core/` workspace crate** — P0-spike scaffolding for the Android IME; kept pending the
  Android product decision (its deletion is a product call, not a cleanup).
- **Clipboard managers capture paste-flow content** — inherent to clipboard-based paste;
  sensitive-phrase opt-out noted under Medium.
- **`nom v1.2.4` future-incompat** (transitive dep) — informational; `cargo update` pass
  before the next minor.
- **AI completion text / imported names rendered with null CSP** — sink absence verified
  today; the CSP item above is the durable fix.

## Missed by pipeline, caught by blind review

The context-blind reviewer (`.pipeline/blind-review.md`) independently confirmed all four
blockers and the majority of High items (noted below where corroborated). These findings
were **not** in the six-dimension audit and are logged here rather than folded in:

- [ ] **Shell commands run synchronously on the key-listener thread** — a slow shell macro
  (`sleep 30`) freezes all keystroke processing, spellcheck, and hotkeys via `maybe_expand`'s
  `block_on` (autotext/expand.rs:203-218). Aggravates blocker #1.
- [ ] **Echo-suppression can wedge permanently** — `quiet_clear`'s second loop is unbounded
  and user keystrokes refresh the echo timer; worse, a worker panic between
  `inj_flag.store(true)` and `quiet_clear` (no catch_unwind) sticks the flag `true` forever —
  autotext, spellcheck, and chord hotkeys all dead until restart with `running: true`
  (expand.rs:393-422, listen.rs:478-486). Sharpens audit High "echo-suppression".
- [ ] **`has_ai` only inspects the top-level body** — a Nested phrase linking to an AI phrase,
  or a Cite macro (Crossref HTTP), blocks the listener thread on the network for up to the
  30s/120s client timeouts (expand.rs:32-36 vs repo.rs:1000-1029).
- [ ] **Sync snapshot edge cases** — a snapshot missing `phrases`/`folders` keys fails
  `from_str` (no `#[serde(default)]`, sync.rs:40-41) and hits the same wipe-as-empty path as
  blocker #4; a single schema-rejected phrase (e.g. `content_type: "html"`) aborts
  `sync_now` mid-apply, permanently braking sync for that folder with no user-visible error.
- [ ] **`ydotool_key` ignores the child's exit status** (inject.rs:150-157) — dead
  `ydotoold` yields silent no-ops with `Ok(())`, same class as blocker #3.
- [ ] **`ai_decide` replace/insert** — clipboard restore skipped when `paste_text` errors
  (commands.rs:537-547); and dismissing the selection-AI popup strands the user's selection +
  prior clipboard in `ai_selection` (lib.rs:255-262).
- [ ] **Clipboard auto-capture has no sensitive-app exclusion** — passwords copied from
  KeePassXC/1Password land in the history DB (plaintext when no keychain exists);
  `spellcheck_exclude_apps` precedent exists (triggers/clipboard.rs:39-56).
- [ ] **`spawn_sync` spawns unbounded queued threads** on rapid mutations (commands.rs:803-810,
  serialized by SYNC_LOCK).
- [ ] **evdev duplicate device nodes double-deliver keys** on combo hardware
  (evdev_listen.rs:99-101).
- [ ] **Pipeline artifacts in the release tree** — `AUDIT.md`, `.pipeline/`,
  `trash2review/` are committed; exclude them from release tags (they're docs-only, but a
  1.7.0 tarball shouldn't ship audit scratch).

### Corroborated (higher confidence)

Both passes independently verified, with matching call-site evidence: blockers 1–4 (the
reviewer additionally found the tombstone *test* masks blocker #2 by hand-stamping a newer
timestamp), `update_phrase` folder_id drop, DEK regeneration, clipboard clobber-on-error,
stale-correction undo, evdev `opened` recovery gap, encryption-toggle swallow, plaintext
sensitive bodies in sync snapshots, CSP+API-key-to-webview, `read_image` escape, LWW
timestamp precision, `run_migrations`/`db_err`/`conn.lock().unwrap()`/tray-quit items,
`import_ahk` test gap, release-workflow test gap, stale `package-lock.json`.

### Disagreements

None material. The reviewer's sharper framing of the echo-suppression wedge (permanent
vs. transient) is adopted above; it raised no audit finding it believed was wrong, and
its "clean" list matches the audit's clean areas.

### Honest limits of this phase

The blind reviewer ran on the same underlying model as the pipeline, so model-level blind
spots are uncorrected. It also located and read `AUDIT.md` despite the sanitized dispatch
(it states it re-verified every finding independently — plausible from its call-site
citations — but full framing-bias removal cannot be claimed). For a genuinely independent
signal, paste a code sample into a brand-new session (or hand it to another reviewer)
with the same cold-review question.

## Pipeline artifacts

- `trash2review/` — 2 superseded docs (`docs/preliminary-report.md`,
  `docs/task-list.md`) + 1 dead module (`desktop/src/lib/stores/resource.svelte.ts`),
  parked for human review; restore with `git mv` per its README.
- `.pipeline/` — phase plans and the blind review transcript.
