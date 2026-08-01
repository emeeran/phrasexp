# phrasexp — desktop (Tauri 2 + Rust + Svelte 5)

A lightweight, local-first text expander in the spirit of PhraseExpress. Type an abbreviation
and it expands into a full phrase; or summon a search popup with a global hotkey. Built as a
native desktop app: a **Rust core** owns storage, triggers, and OS-level input injection, with a
**Svelte 5** webview for the UI.

> See `docs/04-design/adr/0001-tauri-2-desktop.md` for the platform decision and the Linux
> X11/Wayland input strategy.

## Features
- **Phrase library** — folders, phrases, ordered body segments (text + macros), soft-delete,
  sensitive masking. Persisted to local SQLite (`phrasexp.db` in the OS app-data dir).
- **Macros** — `datetime`, `datemath`, `inputform` (constrained, terminating — v1).
- **Two trigger paths:**
  - *Hotkey → popup:* press the global hotkey (default **Ctrl+Alt+Space**) → fuzzy popup near the
    cursor → pick → it types into the focused app.
  - *Silent autotext:* type an abbreviation anywhere → it's erased and the expansion is typed.
- **Clipboard** — auto-captures text copies into a bounded (100) history.
- **Settings** — toggle autotext/clipboard, change the popup hotkey, launch-on-login.
- **Tray** — closing the window hides to tray; the app keeps running.

## Run (development)
```bash
cd desktop
npm install
cargo tauri dev
```
Requires Rust + Node, plus on Linux: `webkit2gtk-4.1`, `libxtst` (for injection). The first build
fetches and compiles dependencies (a few minutes).

## Build (installable)
```bash
cd desktop
cargo tauri build      # produces bundles in src-tauri/target/release/bundle
```

## Platform notes
| Platform | Silent autotext | Hotkey + popup injection | Notes |
|---|---|---|---|
| **Linux / X11** | ✅ rdev (XRecord) | ✅ enigo (XTest) | Works without extra privileges. |
| **Windows** | ✅ rdev | ✅ enigo | Works by default. |
| **macOS** | ✅ rdev | ✅ enigo | Requires **Accessibility** permission (System Settings → Privacy & Security). |
| **Linux / Wayland** | ❌ unsupported | ⚠️ via `ydotool` (uinput) | No portal for global key capture; injection needs `ydotool` + `/dev/uinput`. Hotkeys need the `ashpd` GlobalShortcuts portal. See ADR 0001. |

### Privacy / trust
The silent-autotext listener is a global key handler *by construction*. It matches **only
registered abbreviations** and **never logs, stores, or transmits** raw keystrokes. Clipboard
capture stores copied text locally only. All data stays on-device; there is no network egress.

## Architecture
```
desktop/
  src/                      Svelte 5 SPA (Library, PhraseEditor, Popup, Settings)
    lib/                    typed invoke() wrappers + shared types
  src-tauri/
    src/
      domain/               models, pure expansion logic, errors  (ported 1:1 from backend/app.py)
      db/                   rusqlite repository + migrations  (same schema as the FastAPI app)
      triggers/
        autotext.rs         rdev key listener + abbreviation index
        clipboard.rs        arboard poller
        inject.rs           Injector trait: enigo (X11/Win/macOS) + ydotool (Wayland)
      commands.rs           Tauri command surface (frontend ↔ Rust)
      lib.rs                app setup: settings, hotkey, tray, autostart
    capabilities/           scoped IPC permissions
    migrations/0001_init.sql
```
The companion `backend/` (FastAPI) is the **API/spec reference** — same domain model and SQLite
schema, same invariants. It is not shipped in the desktop binary.

## Verify
```bash
cd desktop && cargo test                 # 22 unit tests (domain, storage, autotext)
cd desktop && cargo clippy --all-targets -- -D warnings   # lint-clean
cd desktop && cargo tauri dev            # manual E2E
```
