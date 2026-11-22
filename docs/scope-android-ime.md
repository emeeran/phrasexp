# Scope: PhraseXp Keyboard — Android IME

Option 3 from the mobile discussion (2026-10-30): a **real** Android text
expander, built as an IME (system keyboard) app whose expansion engine is the
existing Rust core, shipped as a native library. The keyboard *is* the input
path on Android, so it can substitute text as you type — the only architecture
that actually expands (Tauri-on-Android would be a library-manager shell; see
the conversation that produced this doc).

## Architecture

Two artifacts in one repo:

```
desktop/        existing Tauri app (unchanged)
core/           NEW cargo workspace crate — the portable engine (Rust)
android/        NEW Android project — IME service (Kotlin) + thin JNI bindings
```

**Rust → Android via JNI** (uniFFI recommended over hand-written JNI: it
generates Kotlin bindings from the Rust API and handles types/threads).
Build with `cargo-ndk`; targets `arm64-v8a` + `x86_64` (emulator) first,
`armeabi-v7a` only if a device demands it.

### What ports as-is (~6.4K lines)

| Module | Lines | Notes |
|---|---|---|
| `domain/` (models, expand, error) | ~1.2K | `substitute_template`, `Macro` enum, case adaptation. Only `Env`/`File` macros touch `std::env`/fs — sandbox-safe, just low-value on Android |
| `db/` (repo.rs, crypto.rs) | ~3.0K | rusqlite bundled works on Android; DB file moves to app-private storage. crypto.rs's keyring backend swaps to **Android Keystore** (new small backend) |
| `triggers/autotext/{expand,index,spellcheck}` | ~1.9K | Trigger matching, case-adapt, SymSpell + dictionaries (`include_str!` ships fine in the .so) |
| `citation.rs`, `ai.rs` | ~2.2K | reqwest/rustls works on Android; AI stays BYO-key |

**One seam:** `db/repo.rs:26` `read_clipboard_text()` (for the `clipboard`
macro) is the only desktop dep (arboard) inside portable code — make it a
bridge the host app injects; Android supplies `ClipboardManager`.

### What does NOT port

`evdev_listen.rs`, `inject.rs`, `listen.rs`, `offer.rs`, `app_context.rs`,
`triggers/autotext/autotext.rs` glue — all evdev/enigo/window machinery. The
keyboard replaces all of it: on Android the IME *receives* the keystrokes
(that's its job), so no global hooks exist or are needed.

## How expansion works in an IME

1. Keep the last N chars the keyboard committed (or read via
   `getSurroundingText` / `ic.getCursorCapsMode` for case).
2. On each keypress, run the portable trigger index (`index.rs`) on the
   rolling buffer — same delimiter-gated matching as desktop (`sig` fires on
   `sig␣`, not inside "signature").
3. On match: `deleteSurroundingText(trigger_len)` +
   `commitText(expansion)` — with `setComposingText` for the in-flight word so
   autocomplete-style behavior feels native.
4. Case adapt (`BR,` → ALL CAPS) is already pure logic in `domain/expand.rs`.

## Feature mapping (desktop → keyboard)

| Desktop | Android IME | Delta |
|---|---|---|
| Abbreviation autotext | Core feature | Same engine, new trigger surface |
| Search popup (hotkey) | Long-press key or toolbar button → phrase picker sheet | New UI, reuses `db` queries |
| Spellcheck (while typing) | Inline suggestion strip from SymSpell | Latency budget is per-keystroke; index is already keystroke-fast on desktop |
| AI rewrite / citations | Toolbar actions on selected/composing text | BYO key screen needed; network-permission disclosure on Play |
| Input forms (`input` macro) | Bottom-sheet dialog per field | Reuses prompt-once semantics |
| Clipboard capture/history | ❌ Not possible (Android 10+ blocks background clipboard) | Deliberate non-goal |
| Global hotkeys, tray, app-scoping, sync-dir watcher | ❌ / via SAF | App-scoping is meaningless (keyboard sees every app); sync becomes explicit import/export via Storage Access Framework or Syncthing pointing at the app's exported folder |

## Phases

| Phase | Content | Effort |
|---|---|---|
| P0 | Spike: empty IME (Kotlin `InputMethodService` + basic layout), `cargo-ndk` hello-world JNI, CI build for both ABIs | 2–3 days |
| P1 | Extract `core/` workspace crate from `desktop/src-tauri` (move `domain`, `db`, `autotext/{expand,index,spellcheck}`, `citation`, `ai`; feature-gate arboard behind a host bridge; desktop crate keeps working — **all 168 tests stay green on both sides**) | 1 week |
| P2 | Keyboard v1: QWERTY layout, commit path, rolling trigger buffer, expansion + case adapt via JNI | 1 week |
| P3 | Phrase picker sheet, input-form dialogs, pick/random/datetime macros end-to-end | 1 week |
| P4 | Settings app surface (same repo, normal activity): DB lives in app storage; import/export JSON + CSV via SAF; optional Syncthing-folder sync | 3–4 days |
| P5 | Spellcheck strip, AI/citation toolbar actions, Android Keystore for at-rest encryption | 1–2 weeks, optional |

**Total to a useful v1 (P0–P4): ~4–5 weeks** single-dev. P2 end (~2.5 weeks)
is the first moment something genuinely useful exists: type `sig␣`, get your
signature, in any app.

## Open decisions (before P0)

1. **Distribution:** Play Store (IMEs get extra review; AI key entry and
   network need a privacy disclosure) vs sideload/F-Droid APK first
   (recommended: APK first, Play when it's proven).
2. **Sync story:** SAF import/export (simple, manual) vs Syncthing-shared
   folder (matches desktop's file-based sync, more setup). Start SAF.
3. **Rust layout:** workspace crate shared by both targets (recommended) vs
   separate fork of the core (drift — don't).
4. **Min SDK:** 26 (8.0) covers ~97% devices and gives `getSurroundingText`.

## Honest risks

- **Keyboard market:** users are entrenched in Gboard/SwiftKey; a text
  expander keyboard competes as a *second* keyboard you switch to for
  snippets, not a replacement. That's how TextExpander Android works — viable
  but niche.
- **Two UIs, one engine:** every core change now ships on two platforms; the
  workspace-crate layout makes drift impossible to miss (CI builds both).
- **IME review on Play** is stricter (IMEs can log everything you type); the
  local-first, no-telemetry story is genuinely an asset here.
- JNI glue is maintenance surface — keep the FFI boundary small (one facade
  API, not per-function bindings).
