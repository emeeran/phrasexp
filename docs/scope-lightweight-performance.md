# Scope: Lightweight, Fast, High-Value (2026-11 audit)

Goal: cut resource usage (RAM, idle CPU, binary size), speed up every hot path
(keystroke → expansion, popup open → inject, AI run), and add the features with
the best value-to-effort ratio — without dropping any existing feature.

Findings come from three codebase audits (runtime, deps/binary, perf/features).
Effort: S ≤ 2 days, M ~ a week, L more. All paths relative to `desktop/`.

## Baseline (measured 2026-11)

- Binary 29.4 MB unstripped → 9.4 MB deb (`PhraseXp_1.9.0_amd64.deb`)
- Frontend bundle 512 K (brotli-embedded — already fine)
- Deps already lean: reqwest rustls-only (no OpenSSL), tokio time-only,
  keyring on light `dbus-secret-service`, no dup regex/unicode/md stacks
- Threads at startup: app poller (2.5 s), key listener + evdev supervisor
  (3 s rescan), clipboard poller (1.5 s) — all run forever

## Tier 1 — Quick wins (zero-to-low risk)

| # | Change | Impact | Where |
|---|--------|--------|-------|
| 1.1 | `[profile.release]`: `strip = "symbols"`, `lto = "thin"`, `codegen-units = 1` | binary −9–11 MB, deb → ~6–7 MB, free runtime perf | `src-tauri/Cargo.toml` (no release profile exists) |
| 1.2 | Cache parsed `Phrase` (or its chunks) in the abbrev index | removes a DB read + JSON decrypt/parse on EVERY expansion | `src-tauri/src/triggers/autotext/expand.rs:153`, `index.rs:397` |
| 1.3 | Literal matching O(suffix): key abbrevs by last char / reversed trie instead of 2 linear scans per keystroke; anchor regex triggers at buffer tail | per-keystroke cost stops scaling with library size | `index.rs:21-40, 160-163, 386-394` |
| 1.4 | Stop full-table decrypt passes: `app_poll_needed` via `SELECT EXISTS(app_scope)`, lighter reload queries | 3 full-library decrypts per phrase edit → 1 | `lib.rs:91-109, 438-461` |
| 1.5 | Popup: no whole-library reload on every focus gain (serve from webview cache + `library-changed` event invalidation); `get_phrases(ids)` batch for the chooser | faster popup open at 1000+ phrases | `src/lib/components/popup/Popup.svelte:521-528, 184` |
| 1.6 | Debounce Library search; drop redundant server-side `q` filter (client `rankPhrases` already covers it); fix double `fuzzyScore` call | Library stops firing an IPC per keystroke | `src/lib/components/editor/PhraseList.svelte:91`, `src/lib/fuzzy.ts:102-103` |
| 1.7 | Debounce/coalesce `spawn_sync`; skip push when nothing changed | one edit currently rewrites the whole library JSON | `commands.rs:54-63, 831-837`, `sync.rs:136-241` |
| 1.8 | Move `record_usage` off the listener thread (into the inject worker); drop `block_on` from the sync expansion path | WAL write + runtime overhead out of the hot path | `expand.rs:208, 240-241, 347, 364` |
| 1.9 | Delete `device_query` (use Tauri `cursor_position()`); `pulldown-cmark` default-features off; `thiserror` 1 → 2 | −4 crates + dup stacks, compile-time win | `lib.rs:315-321` |
| 1.10 | Memory hygiene: cap `LAST_DELIVERY`/AI-slot retention (skip > 64 KB, clear on hide); `prepare_cached` on `list_phrases`/`load_settings`; single `DELETE` for clipboard eviction | bounded memory, one-liners | `inject.rs:326`, `repo.rs:579, 710-740, 1221` |

## Tier 2 — Structural (medium effort)

1. **Spellcheck index memory (biggest consumer)** — SymSpell deletes index over
   25K words at edit-distance-2 ≈ 1–3 M map entries (plausibly hundreds of MB),
   built on first typed space, never freed. Build only when spellcheck enabled;
   max_edit=1 static + fallback; interned word-id keys.
   (`triggers/autotext/spellcheck.rs:211-241`)
2. **Event-driven pollers** — clipboard via X11 `XFixesSelectionNotify` (or
   5–10 s backoff + skip when disabled); inotify for `/dev/input` rescans;
   lazy-start the xdotool poller; pause the 2 s frontend status poll when the
   main window is hidden. Removes all idle wake-ups.
   (`triggers/clipboard.rs:13`, `evdev_listen.rs:33`, `lib.rs:135-146`,
   `src/lib/stores/status.svelte.ts:23`)
3. **Injector reuse + no sleep-polling** — long-lived Enigo / persistent
   `ydotoold` socket; replace `quiet_clear` (500 ms of 10/15 ms sleeps per
   injection) with a condvar fed by the listener's echo timestamps; kill the
   fixed 120 ms settle in every popup inject path.
   (`inject.rs:49-114`, `expand.rs:402-431`, `Popup.svelte:231`)
4. **Settings/AiAction caching** with an invalidation hook instead of per-run
   DB reads (incl. keychain access) — `repo.rs:1010-1028, 1221-1306`,
   `commands.rs:641-645`.
5. **Startup: get out of the main thread's way** — move keychain init,
   abbrev-index build, and `migrate_encrypt_at_rest` to a background thread;
   build tray + show window immediately. (`lib.rs:597-833`)
6. **AI streaming render** — append `ai-delta` chunks instead of re-setting the
   accumulated string (O(n²) text churn on long responses).
   (`Popup.svelte:563-567`)

## Tier 3 — Value-adding features (ranked by value/effort)

1. **Encrypted sync snapshots** (M) — sync currently writes decrypted bodies to
   a plaintext JSON file; passphrase-sealed snapshot is the biggest trust win
   vs Espanso/TextExpander. (`sync.rs:227-239`)
2. **Usage statistics dashboard** (S–M) — `expanded_count` is collected on every
   expansion but barely shown; trends, most/never-used, "clean up unused"
   action. Pure UI on existing data.
3. **Export to Espanso / TextExpander / CSV** (S–M) — importers exist, exporters
   don't; migration away is currently impossible.
4. **Safe shell macros** (M) — timeout + output cap (a hung command wedges the
   worker forever), and carry script snippets in sync (locked) instead of
   silently dropping them. (`domain/expand.rs:79-92`, `sync.rs:177-186`)
5. **Editor live preview + abbreviation-conflict warning at save** (M).
6. **Deeper app scoping** (M–L) — per-phrase exclude lists, event-driven window
   tracking (removes the stale 2.5 s poll), Wayland-native source.
7. **FTS5 search** (M) — one index powering Library + popup.

## Already optimal (do not touch)

reqwest (rustls, webpki-roots) · tokio (time-only) · rdev + hand-rolled evdev
(X11 fallback + Wayland — can't trim) · enigo (sole inject path) · frontend
bundle · keyring backend · tauri `compression` (size win for embedded assets)

## Suggested sequence

1.1 → 1.2 → 1.3 (compound: smaller binary, faster keystroke, faster expansion)
→ 1.5 popup reload → rest of Tier 1 → Tier 2 by number → Tier 3 by value.
