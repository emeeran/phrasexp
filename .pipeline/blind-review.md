# Blind review — verbatim transcript (2026-11-21)

Reviewer dispatch: sanitized prompt (cold due-diligence framing; no mention of the
cleanup pipeline, phases, trash2review, or AUDIT.md). Sanitization caveat, disclosed
honestly: the reviewer found the committed AUDIT.md on its own and noted it did not
trust it and re-verified everything independently. It claims and demonstrates
independent verification (its own call-site checks), but full framing-bias removal
cannot be claimed. As with any model-based review, this does not substitute for a
fresh-session human review.

---

# PhraseXp due-diligence review

**Context note:** the repo already contains an `AUDIT.md` written today (commit `524d8f6`). I did *not* trust it — I re-read the code and independently verified every finding below before reporting. I confirm most of it, found several things it missed (some more severe than what it has), and disagree with one of its severity calls. I've marked what I verified for each item.

---

## Blockers

### B1. `Macro::Shell` / `File` / `Env` are reachable from two untrusted inputs — arbitrary code execution on typing
`desktop/src-tauri/src/domain/expand.rs:79-92` runs `sh -c <command>` (Windows: `cmd /C`) with no gate, no timeout, no prompt; the comment at `expand.rs:80-81` explicitly says "add a timeout if imported libraries are ever untrusted" — they are untrusted today. Verified end-to-end:

- **JSON import:** `commands.rs:1313-1476` (`import_json`) deserializes full `Phrase` values (`models.rs:407-416` `Segment::Macro` wraps `models.rs:101-149` `Macro`, both derive `Deserialize`), stores them via `create_phrase`, and `after_library_change` (`commands.rs:49-57`) rebuilds the index — the phrase fires the next time the abbreviation is typed. No filtering of dangerous macros anywhere on this path (contrast `simple_phrase` at `commands.rs:1500-1532`, which the CSV/AHK/Espanso paths use and which only builds `Segment::Text`).
- **Sync:** `sync.rs:128,153-155` merges every phrase in the snapshot via `upsert_synced_phrase` (`repo.rs:486-530`) with no macro filtering. LWW means a hostile or compromised `phrasexp-library.json` (typical folder: Dropbox/Syncthing/WebDAV) can *replace* existing phrases with `Shell` variants.
- `Macro::File` (`expand.rs:52-53`) and `Macro::Env` (`expand.rs:44`) are the same vector short of RCE: read an arbitrary file/env var, type it into any window — an exfiltration primitive.

Additional impact I verified beyond the audit: a slow shell command (`sleep 30`) executes **synchronously on the key-listener thread** via `maybe_expand`'s `block_on` (`autotext/expand.rs:203-218`), freezing all keystroke processing, spellcheck, and global hotkeys for the command's duration.

### B2. Soft-delete never bumps `updated_at`, so sync tombstones never win — deletions silently undo and flip-flop
Verified: `repo.rs:627-631` (`soft_delete_phrase`) sets only `deleted_at`; there are no SQLite triggers (grepped `migrations/0001_init.sql`); `merge_lww` compares `updated_at` only (`sync.rs:63`). Consequence: device B's live copy always beats device A's older-stamped tombstone on the next push, and the deleted phrase resurrects forever. `merge_folders` has the same missing-bump (`repo.rs:367-378`), so folder moves never propagate either. The in-repo test `sync.rs:233-241` masks this by hand-stamping the tombstone with the *newer* timestamp — something `soft_delete_phrase` never does.

### B3. Corrupt or partially-invalid sync snapshot is treated as "empty remote" and then overwritten — cross-device data destruction
Verified at `sync.rs:118-124`: `read_to_string().ok()` + `from_str().ok()` collapse *missing*, *transient read error*, and *corrupt* into "no remote", and `sync_now` unconditionally writes the merged (local-only) snapshot back (`sync.rs:159-171`). Startup sync runs 3s after launch (`lib.rs:825-833`), so the wipe can happen before the user ever opens Settings. Two aggravations I found that the existing audit missed:

- A snapshot whose JSON is valid but missing `phrases`/`folders` keys fails `from_str` (no `#[serde(default)]` on `sync.rs:40-41`) → same wipe path.
- A snapshot containing a single phrase the local schema rejects (e.g. `content_type: "html"` — `Phrase::validate` at `models.rs:539-547` returns `ReservedContentType`, and `upsert_synced_phrase` validates at `repo.rs:487`) aborts `sync_now` mid-apply (`sync.rs:153-155` `?`) *after* some phrases were applied and *before* the snapshot is written. Every subsequent sync fails the same way: sync is permanently braked for that folder with no user-visible error (only `debug_log`, which is stderr-only).

### B4. Injection failures are silent and corrupt state
Verified: every enigo result is discarded in `EnigoInjector` — `inject.rs:50,57,98-101,108,117,130-133` (`let _ = enigo.text(...)` etc.), so `type_text`/`backspace` return `Ok(())` after doing nothing. `ydotool_key` (`inject.rs:150-157`) checks only spawn success and ignores the child's exit status — on Wayland a not-running `ydotoold` yields silent no-ops with `Ok(())`. All call sites swallow delivery errors: `autotext/expand.rs:185,245,366`, `listen.rs:608-609`, `lib.rs:272,370`. Because the failures are `Ok`, `LAST_DELIVERY` (`inject.rs:357-360`) records a delivery that never happened (repeat-last-output re-fires garbage) and `record_usage` counts phantom expansions. Combined with B6 (no log surface), the headline failure mode — "typed my abbreviation, nothing happened" — is undiagnosable.

---

## High

### H1. Echo-suppression flag can wedge permanently, killing all typing features; `quiet_clear` has no timeout
This is sharper than the audit's version. Verified chain: `inj_flag.store(true)` at `autotext/expand.rs:229` (also `137-139,163-165,270`); the worker then runs `backspace → deliver_chunks → quiet_clear` (`expand.rs:243-247`). `quiet_clear` (`expand.rs:393-422`) has a bounded first wait but an **unbounded second loop** — and while the flag is set, the listener drops *all* keys and refreshes `last_echo` on every one of them (`listen.rs:478-486`), including the user's real keystrokes, so the loop can spin indefinitely. Worse: if the worker thread panics between `store(true)` and `quiet_clear` (no `catch_unwind` anywhere), the flag is stuck `true` forever → autotext, spellcheck, chord hotkeys all dead until restart, `autotext_status` still reports `running: true`, and nothing is logged.

### H2. `update_phrase` silently drops folder moves while reporting success
Verified end-to-end: the UPDATE at `repo.rs:598-614` lists ten columns — no `folder_id`. The frontend does send it: `PhraseEditor.svelte:319` renders a folder `<select>` bound to `editFolderId` (in *edit* mode too, not just create), `phraseDraft.ts:55,72` puts `folder_id` in the payload, `commands.rs:1895` applies it onto the phrase, and the command returns that phrase — so the UI shows the move while the row keeps the old folder. This is a "user did an action, app lied about it" bug.

### H3. Corrupt keychain DEK silently regenerates — every `enc1:` row becomes a locked placeholder
Verified `db/crypto.rs:87-88`: a stored DEK of the wrong shape falls through to `Self::generate`, overwriting the keychain entry. All sensitive bodies (`repo.rs:410-415`), clipboard history (`repo.rs:672`), and the AI key (`repo.rs:1250-1257`) written under the old DEK are permanently unreadable, rendered as `"[encrypted — keychain unavailable]"` (`repo.rs:76`). Data-loss-with-no-warning; should refuse and surface.

### H4. Clipboard clobber on failed paste; clipboard restore is best-effort
Verified `inject.rs:244-259`: if `set()` succeeds and `paste()` fails, `?` returns at `inject.rs:252` and the user's clipboard is left holding the expansion (potentially a sensitive phrase) permanently. Only clipboard *text* is saved (`get_text()` at `inject.rs:250`) — an image clipboard is destroyed even on success. Also, `types_reliably` (ASCII ≤ 200, `inject.rs:394-396`) sends short secrets through `type_text`, which lands them in X primary selections/IM buffers more broadly than paste would.

### H5. Stale-correction undo has no recency bound
Verified `listen.rs:582-614`: `last_correction` is cleared only after the user backspaces and then completes another word (`listen.rs:778-793`); otherwise it survives indefinitely. Pressing Left arrow minutes later — in a different document — backspaces `corrected.len()+1` characters and retypes the original at the current caret, corrupting unrelated text. Needs a keystroke/time staleness bound.

### H6. evdev reader recovery is broken by the `opened` set
Verified `evdev_listen.rs:55-68`: dead readers are removed from `handles` (`:58`) but their nodes stay in `opened` forever; the hotplug rescan skips any node in `opened` (`:62-63`). A device that comes back on the *same* node path (common after resume/KVM switches) is never reopened; only the all-readers-dead branch (`:72-84`) recovers. Also no `catch_unwind` on the ~20 spawn sites (`listen.rs:461,876`, `evdev_listen.rs:102`, `lib.rs:210,827`, `commands.rs:800`) — one callback panic (`listen.rs:474`) kills that device's reader permanently while `autotext_status` keeps reporting `running: true` (`listen.rs:849` runs only on normal return).

### H7. Encryption-toggle migration failure fully swallowed
Verified `commands.rs:1861-1869`: both `migrate_encrypt_at_rest()` and `decrypt_at_rest()` results are `let _ =`-ed, leaving a mixed plaintext/cipher library after a "successful" settings save. The startup path at least `eprintln!`s (`lib.rs:635-639`).

### H8. Sync snapshot writes decrypted sensitive bodies in plaintext to the sync folder
Verified: `sync_now` reads local phrases via `list_phrases` (`sync.rs:126`), which returns *decrypted* bodies (`repo.rs:545-586` → `row_to_phrase` → `decode_body_json`), and serializes them straight into the JSON written to the user-chosen (typically cloud-synced) folder. `export_library` documents this risk (`commands.rs:838-845`, `sensitive_count`); sync has no such warning, and `sync_set_dir` (`commands.rs:816-823`) accepts any path with no notice.

### H9. Decrypted AI API key is handed to both webviews, with `csp: null`
Verified: `get_settings` (`commands.rs:1836-1839`) returns `load_settings()`, which decrypts `ai_api_key` (`repo.rs:1250-1257`); the capability set covers both `main` and `popup` (`capabilities/default.json:4`); `tauri.conf.json:36` sets `csp: null`. There is no `{@html}`/`innerHTML` sink today (I grepped — clean), so this is not currently exploitable, but the popup webview (which renders imported phrase names, AI output, error strings) has no CSP backstop and receives the raw API key via `getSettings()`. Set a strict CSP; mask the key in `get_settings` and make the frontend send `""` to mean "unchanged".

### H10. `read_image` path check is lexically escapable
Verified `commands.rs:217-224`: `Path::starts_with` is a component-wise prefix test, so `/…/images/../anything.png` passes (`..` is just a component) and the `png` extension check holds — the webview can read any `*.png` on disk as a data URL. Canonicalize (`std::fs::canonicalize`) and re-check. Also no size cap on `save_image` (`commands.rs:194-211`) or import payloads (memory DoS from the webview).

### H11. Nested/Cite AI expansions block the key listener on the network
Verified: `has_ai` (`autotext/expand.rs:32-36`) only inspects the *top-level* body. A phrase containing `Macro::Nested { … }` that links to an AI phrase, or a `Macro::Cite` (Crossref HTTP), reaches `expand_inner`'s awaits (`repo.rs:1000-1029`) via the **synchronous** `block_on` path (`autotext/expand.rs:203-218`) — i.e., on the listener thread. Typing such an abbreviation freezes global key processing for the full request duration (up to the 30s/120s client timeouts, `ai.rs:65,116`). `has_ai` should recurse (or the chunk expansion should always run on a worker).

### H12. Release tags run zero tests
Verified `.github/workflows/ci.yml:7-10` (push to `main` + PRs only) and `release.yml:8-10` (tags only): a tag cut on a broken main ships installers for three OSes with no gate.

---

## Medium

- **LWW timestamp comparison mixes RFC-3339 formats.** `now_iso()` (`domain/mod.rs:14`) emits `…+00:00` with *variable* fractional digits; `snapshot_timestamp()` (`sync.rs:96`) emits `…Z` with fixed millis. String comparison (`sync.rs:63`) mis-orders same-second sub-millisecond collisions because `'Z' > '+'` and digits — a snapshot stamp at `.000Z` beats a *newer* local `.000000001+00:00`. Practically narrow, but the honest fix is parsing to `DateTime` before comparing.
- **`reset_database` doesn't touch the sync snapshot** (`repo.rs:737-745`) — the next `sync_now` resurrects everything the user just erased. Also not wrapped in a transaction with the index rebuild.
- **`run_migrations` ignores every `ALTER TABLE` error** (`repo.rs:216-260`, `let _ =`): a real failure (locked DB, disk full) is indistinguishable from the expected duplicate-column error, later writes then fail confusingly.
- **~60 × `conn.lock().unwrap()`** (e.g. `repo.rs:214,587,621…`): one panic while holding the DB mutex poisons it and every subsequent DB call panics process-wide. Everything else in the codebase tolerates poisoning with `into_inner()` (`sync.rs:106`, `evdev_listen.rs:116`, `listen.rs:845`) — the DB should too.
- **`ai_decide` "replace/insert" pastes before restoring the clipboard** (`commands.rs:537-547`): if `paste_text` errors after the result slot was already cleared (`:538`), `old_clipboard` is never restored.
- **Stale selection-AI slot:** dismissing the popup without running an action leaves `ai_selection` holding the user's selection + prior clipboard (`lib.rs:255-262`); the clipboard is restored only on the next capture or an explicit discard (documented at `lib.rs:72-74`, but the user's clipboard is gone until then).
- **Clipboard auto-capture has no sensitive-app exclusion** (`triggers/clipboard.rs:39-56`): passwords copied from KeePassXC/1Password land in the DB (encrypted at rest only if a keychain exists — `crypto.rs:91` degrades to plaintext otherwise) and in the Clipboard page. `spellcheck_exclude_apps` exists; capture deserves the same list.
- **Imported-folder creation swallowed** (`commands.rs:1405` `let _ = create_folder`): deferred import folders can vanish while the import reports success.
- **`db_err` flattens all SQL errors to `Validation`** (`repo.rs:20-22`): frontend can't distinguish "database is locked" from "invalid input".
- **Tray Quit is a bare `app.exit(0)`** (`lib.rs:790-793`): no drain for in-flight injections (mid-`deliver_chunks` quit = erased abbreviation, half-typed output) or the startup sync's read-modify-write of the snapshot (B3's temp+rename helps, but a quit between apply and write silently skips the push).
- **`import_ahk` has zero tests** (`commands.rs:1117-1153`) — the only import parser without a dedicated suite; its inline-comment heuristic (`split("  ;")`) and `::trigger::replacement` scanning are exactly the kind of thing that regresses silently.
- **`package-lock.json:3` says `1.3.1`** while `package.json`/`tauri.conf.json` say `1.7.0` — verified; the lockfile hasn't been regenerated since the bump.
- **`spawn_sync` on every library mutation** (`commands.rs:803-810`): rapid multi-edit (import loops call `after_library_change` once, fine, but programmatic bulk edits would) spawns unbounded threads; `SYNC_LOCK` (`sync.rs:101`) serializes them so they queue, each doing a full read-merge-write of the whole library.

## Low

- No persistent log surface at all: `debug_log` is stderr-only behind `PHRASEXP_DEBUG=1` (`lib.rs:115-121`), which is **undocumented** in README/docs (verified by grep) and, when enabled, logs typed buffers (`listen.rs:489-492`) — worth a docs note and a scrub.
- Abbreviations > 64 chars can never fire (`MAX_BUFFER` at `listen.rs:23`); no `validate()` length check — create succeeds, trigger never does. `key_to_char` is US-layout only; unmapped keys silently clear the buffer (`listen.rs:814-817`).
- Invalid regex abbreviations are dropped at index build with no warning (`index.rs:88-91`).
- evdev duplicate device nodes (laptop + USB combo interfaces) double-deliver the same key; acknowledged only in a debug-log comment (`evdev_listen.rs:99-101`) — a real double-buffer hazard on such hardware.
- `spellcheck_learned` and the ignore list are unbounded (`repo.rs` `add_spellcheck_learned`); `clipboard_max_entries = 0` is an advertised *unlimited* history of everything copied.
- `grab_keyboard` retries then only `eprintln!`s (`commands.rs:1742-1751`), bypassing the `debug_log` facility; the success line is leftover noise.
- Repo hygiene: `AUDIT.md`, `.pipeline/` (phase plans, review transcripts) and `trash2review/` are committed to the production tree; `core/` is a 35-line `ping()` spike and `android/` a P0 experiment — fine as experiments, but they should not ride along in release tags unchecked (android.yml builds on every push/PR).

---

## Areas I checked and found clean

- **SQL injection:** all statements in `repo.rs` use `params!`/bound columns; no string-interpolated SQL found.
- **Crypto primitives:** AES-256-GCM with per-message random nonce, tag-auth failure closed (`crypto.rs:126-156`), truncated-envelope rejection (`:151`), `Zeroizing` DEK, correct `enc1:` prefix versioning. The tamper/truncation/wrong-key tests are real.
- **AI endpoint gate:** https enforced for remote, loopback http allowed, IPv6-bracket handling correct (`ai.rs:13-26,184-194`); SSE parsing drains partial lines correctly (`ai.rs:135-156`); cancel semantics coherent.
- **Citation:** fixed `api.crossref.org` host (`citation.rs:158-160`) — no SSRF; DOI interpolation unencoded is cosmetic.
- **XSS sinks:** no `{@html}`/`innerHTML`/`eval` anywhere in `desktop/src` (grepped).
- **Secrets:** no hardcoded credentials in source (only test fixtures).
- **Tauri surface:** capability list is minimal (no fs/shell/http plugins); single-instance plugin registered first (`lib.rs:556`); `save_image` generates its own UUID paths.
- **Espanso/AHK/aText/TextExpander imports:** all route through `create_import_phrase` → `simple_phrase` (`commands.rs:1094-1112,1500-1532`) which only ever produces plain text + autotext triggers — the dangerous-macro problem is exclusively `import_json` and sync (B1).
- **Sync FK handling:** unknown-folder backfill prevents FK violations (`sync.rs:130-152`), and folder upserts precede phrase upserts.
- **`merge_lww` pure logic** is correct for the cases its tests cover (newer-wins, local-newer-untouched, remote-import, remote-tombstone, local-tombstone-survives).
- **Frontend popup plumbing** (`Popup.svelte`): busy-guards, `settleThen` hide→120ms→inject ordering, grab release on every offer dismiss path — careful code, better than typical.
- **Spellcheck privacy:** sensitive phrase bodies are excluded from the spellcheck vocabulary index (`index.rs:84-87`) with a correct rationale.

## My overall judgment

The architecture (pure domain core, parameterized SQLite, session-aware injectors, capability-minimal Tauri config) and the test discipline (168 Rust + 42 frontend tests on the pure paths) are genuinely good. But the app has a **systemic pattern**: *silent failure everywhere* (`let _ =` at ~30 critical sites, errors flattened to `Ok` or `Validation`) on a product whose entire job is invisible background typing — and **two untrusted inputs** (imported JSON, the sync folder) that terminate in `sh -c`. I would not ship until B1–B4 and H2/H3 are fixed; H1/H5/H6 are the top on-call risk because they produce the exact "PhraseXp went dead" support ticket with no diagnostic trail.
