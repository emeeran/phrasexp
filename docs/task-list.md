# Task List: Lightweight / Fast / High-Value

Companion to `scope-lightweight-performance.md` (item ids match its tables).
Ordered by the suggested sequence; check off as you go.
Effort: S ≤ 2 days, M ~ a week. Gates: `cargo test` + `npm run test` +
`npm run check` green after every task; clippy clean.

## Phase 0 — Baseline & safety net

- [x] Record baseline numbers: binary 29.4 MB unstripped, deb 9.4 MB, keystroke match cost **221 µs at 5,000 abbreviations** (measured by the regression guard before the O(suffix) fix; RSS/popup-latency live numbers still pending a running-app session)
- [x] Add an expansion-latency smoke test or benchmark marker — `suffix_match_stays_cheap_at_library_scale` (index.rs tests): 2,000 non-matching lookups over a 5,000-abbrev index, generous bound, fails on any O(n) regression

## Phase 1 — Tier 1 quick wins

- [x] **1.1** `[profile.release]`: `strip = "symbols"`, `lto = "thin"`, `codegen-units = 1`; rebuild deb, record new sizes — **binary 29.4 MB → 17.8 MB (−40%)**; deb rebundle pending next release
- [x] **1.2** Cache parsed `Phrase`/chunks in `AbbrevIndex`; expansion reads from the index, DB only on cache miss; invalidate on rebuild (`index.rs`, `expand.rs:153`) — **sensitive phrases deliberately NOT cached** (at-rest bodies stay non-resident; they take the per-expansion DB path)
- [x] **1.3** O(suffix) literal matching (suffix-length walk over the literals map, `best_literal_suffix` rewritten); regex triggers windowed to a 256-byte tail; on-match clones kept (rare path, signatures untouched) (`index.rs`)
- [x] **1.4** `refresh_app_poll_needed` → `has_scoped_phrase()` (`SELECT EXISTS`); index rebuild / hotkey reload keep full reads (the index genuinely needs bodies for spellcheck vocab + phrase cache) (`repo.rs`, `lib.rs`)
- [x] **1.5** Popup serves phrase list from webview cache; reload only on a `library-changed` event (emitted from `after_library_change`); `get_phrases(ids)` batch command for the chooser (`Popup.svelte`, `commands.rs`, `invoke.ts`)
- [x] **1.6** Library search: client-side only (`rankPhrases` recomputes per keystroke from the store query, no IPC); server `q` filter dropped; double `fuzzyScore` call fixed (`PhraseList.svelte`, `phrases.svelte.ts`, `fuzzy.ts`)
- [x] **1.7** Debounce/coalesce `spawn_sync` (2 s trailing window, single worker, exit-race double-check); skip push when content hash unchanged (`LAST_PUSHED_HASH`, folders+phrases only so `exported_at` can't defeat it) (`commands.rs`, `sync.rs`)
- [x] **1.8** Move `record_usage` into the inject worker (all four sites); *block_on removal deferred* — `expand_chunks_with_clip` stays async because `{cite}`/clipboard macros can resolve network, so dropping the executor needs a Db::expand restructure (Tier 2 territory) (`expand.rs`)
- [x] **1.9** Drop `device_query` for Tauri `cursor_position()`; `pulldown-cmark` `default-features = false` + explicit `html` feature; `thiserror` 1 → 2 (`lib.rs`, `Cargo.toml`)
- [x] **1.10** Cap `LAST_DELIVERY` retention at 64 KB (bigger deliveries clear it); `prepare_cached` on `list_phrases`/`load_settings`; single-statement clipboard eviction (`inject.rs`, `repo.rs`). *AI-slot cap skipped: slots already clear on every decide/discard path.*
- [ ] Re-measure baselines vs Phase 0; note deltas in the scope doc *(binary −40% done; RSS/popup latency pending a live session)*

## Phase 2 — Tier 2 structural

- [x] **2.1** Spellcheck index compaction — **measured 102.7 MB → 23.8 MB RSS (−77%)** via three changes: (a) delete-map keys became FNV-1a u64 hashes (keys are only exact-looked-up, never recovered; lookup verifies true Damerau-Levenshtein so collisions are harmless), (b) Soundex buckets hold word indices instead of duplicated strings, (c) `freeze()` compacts the build-time pairs into a flat CSR (sorted `Vec<(hash,start,len)>` + concatenated entries — a per-key `HashMap<u64,Vec<u32>>` intermediate was tried and measured WORSE (99.5 MB) because glibc never returns the freed pages; the flat-pairs build has no fat intermediate). Guard: `baseline_index_sizes` diagnostic prints RSS before/after. Spellcheck behavior covered by the existing 194-test suite incl. golden typo set
- [x] **2.2** Poller work — frontend: 2 s status poll pauses while the main window is hidden to tray (+ one poll on refocus). App poller/clipboard poller were already flag-gated (cheap ticks when not needed); the XFixes event-driven clipboard + inotify `/dev/input` rescan stay in **Backlog** — LOW impact per audit, real OS-API work
- [x] **2.3** Injector reuse + no sleep-polling — one process-global `Enigo` behind a `Mutex` (was a fresh X display connection per operation; `retype_delimiter` reuses it too); `quiet_clear` now sleeps on a condvar the listener pings per echo (was 10/15 ms sleep-polling up to ~500 ms per injection). *The 120 ms popup settle deferred — needs live target-app testing; flagged in Backlog*
- [x] **2.4** Settings cache in `Db` (`Mutex<Option<Arc<Settings>>>`): `load_settings` hits the cache and skips the 26-column SELECT + OS-keychain API-key decrypt that ran per AI macro per action run; invalidated on `save_settings`, spellcheck-ignore writes. AiAction per-run SELECT is cheap — deliberately not cached
- [x] **2.5** Startup: index build + `refresh_app_poll_needed` moved to a background thread (was a full-library decrypt blocking `setup()`); tray/window/main-thread hotkeys live immediately. The listener runs from the start (empty index = no expansion for the sub-second load). *Keychain init + encrypt-migration deliberately kept on main — moving them races the webview's first encrypted reads*
- [x] **2.6** AI streaming: Rust emits per-token deltas, the popup appends (accumulator cleared on run start/retry/chip re-run) — was re-assigning the whole accumulated string per token, O(n²) IPC + render churn

## Phase 3 — Tier 3 features

- [x] **3.1** Encrypted sync snapshot: `sync_passphrase` setting (envelope-encrypted at rest like the AI key; new migration column); snapshot sealed with AES-256-GCM under a PBKDF2-HMAC-SHA256 key (600k iterations, cached per passphrase); file form `PHRASEXP-ENC1:<base64(nonce‖ct)>` with the prefix as AAD. Legacy plaintext snapshots still read; an encrypted snapshot without the passphrase is a NAMED refusal (never treated as empty — that would overwrite the folder with a local-only push); wrong passphrase = named error. Skip-write hash includes the sealing mode so toggling the passphrase always rewrites. UI: passphrase input + "Seal" in Settings → Data → Sync, with the same-passphrase-on-every-devices hint. Tests: roundtrip, wrong-passphrase, fresh-nonce. Deps: pbkdf2 + sha2 (small)
- [x] **3.2** Usage stats: DataPanel card shows top-5 most used + never-used count; "Review never-used" modal (mirrors the dupes modal) with per-row and bulk delete, refreshes library stores after deletion. Per-folder counts and trends deferred until the data earns them
- [x] **3.3** Exporters: `export_espanso` (serde_yaml `matches:` block), `export_textexpander_csv` (official 3-column header, round-trips through `import_textexpander`), `export_csv` (round-trips through `import_csv`). Text-only snippets — macros/images/regex triggers are skipped and the count is surfaced in the toast. Buttons in Settings → Data → Backup; pure formatting helpers unit-tested incl. CSV round-trip through the real parser
- [x] **3.4** Shell macro hardening: 5 s timeout (poll-wait, child killed + reaped on expiry, error names the timeout) + 64 KB stdout cap (capture stops past the cap; child not killed for it) + nonzero exit surfaces as an error (was silently Ok). *Sync carry of script snippets deliberately NOT done — only after the encrypted-snapshot work (3.1), since plaintext sync of scripts would widen the attack surface.* Tests: hung-command timeout, nonzero exit
- [x] **3.5** Editor live preview (`bodyPreview` in `$lib/editor/phraseDraft.ts` — text verbatim, macros as `{label}` placeholders, images as `[image]`, unit-tested) rendered in a dashed preview box under the body editor as you type; abbreviation-conflict warning at save — saving an abbreviation another live phrase owns confirms with the clashing phrase's name and the chooser consequence (advisory: never blocks saving on a lookup failure)
- [ ] **3.6** App scoping v2 — **deferred (M–L)**: per-phrase exclude lists need a column + keystroke-path integration; event-driven window tracking and a Wayland source are OS-API projects of their own (see Backlog)
- [ ] **3.7** FTS5 — **deferred (M)**: after 1.6 the Library search is client-side fuzzy over summaries (one IPC per folder change, no per-keystroke cost) and the popup ranks in-memory; FTS5 only pays off at multi-thousand-phrase libraries or when body-text search becomes a demand. The bundled SQLite already ships FTS5, so it's an index-maintenance + query-plumbing project, not a dependency problem

## Backlog — deferred deliberately

- [ ] **3.6** App scoping v2 (per-phrase exclude lists, event-driven window tracking, Wayland source) — M–L, own project
- [ ] **3.7** FTS5 body-text + ranking search — M, only pays off at multi-thousand-phrase scale
- [ ] Clipboard poller → X11 `XFixesSelectionNotify` / inotify for `/dev/input` rescan — LOW impact (polls are flag-gated and cheap), real OS-API work
- [ ] 120 ms popup→inject settle — event-driven or adaptive; needs live target-app testing (each simulated attempt risks paste races)
- [ ] `expand_chunks_with_clip` de-async (drops `block_on` from the keystroke path) — `{cite}`/clipboard macros resolve network inside `Db::expand`; needs a restructure separating pure expansion from network macros
- [ ] Mobile/companion bridge (Espanso export push) — L, wait for demand
- [ ] Richer forms (multi-field dialog, persisted defaults) — M–L
- [ ] Locale-aware date macros (named formats, localized month names) — S, fold into any date-macro work
- [ ] Virtualize `PhraseList` — only if users report lag at multi-thousand phrase counts (1.5/1.6 likely suffice)
- [ ] `panic = "abort"` + png dedup — revisit at next release-candidate build
