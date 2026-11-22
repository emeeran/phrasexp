# PhraseXp — desktop (Tauri 2 + Rust + Svelte 5)

A lightweight, local-first text expander in the spirit of PhraseExpress. Type an abbreviation
and it expands into a full phrase; or summon a search popup with a global hotkey. Built as a
native desktop app: a **Rust core** owns storage, triggers, and OS-level input injection, with a
**Svelte 5** webview for the UI.

## Features
- **Phrase library** — folders (create / mute / delete / **merge**), phrases, ordered body
  segments (text + macros), soft-delete, sensitive masking, duplicate finder with bulk
  cleanup, and version history with restore. Persisted to local SQLite (`phrasexp.db` in the
  OS app-data dir).
- **Macros** — 11 dynamic segment types: `datetime`, `datemath`, `inputform`, `random`,
  `math` (meval expressions), `clipboard`, `nested` (linked phrases), `env`, `pick`, `file`,
  and `ai` (LLM generation).
- **Two trigger paths:**
  - *Hotkey → popup:* press the global hotkey (default **Ctrl+Alt+Space**) → fuzzy popup near the
    cursor (subsequence search, abbreviation-first ranking, usage-count tiebreak) → pick → it
    types into the focused app.
  - *Silent autotext:* type an abbreviation anywhere → it's erased and the expansion is typed.
    Per-trigger **execute mode**: immediately (default) or after a delimiter (Space/Enter/Tab/
    `,` `.` `;` `:` `!` `?`) — the delimiter is erased and re-typed around the expansion, so
    `sig` inside "signature" never fires. Case adaptation is per-phrase (match-typed /
    always / never) with a global default.
- **Usage stats** — every expansion counts (`expanded_count`, `last_used_at`); phrase rows show
  a ×N badge, the library sorts by Most used, and Settings → Data lists the top phrases.
- **Spellcheck** — auto-corrects typos as you type using Damerau-Levenshtein distance over a
  BK-tree (O(log n) lookup), with Soundex phonetic fallback, frequency-weighted ranking, and
  case preservation. Three aggressiveness modes (conservative / normal / aggressive), an
  ignore-words list, a configurable toggle hotkey (default **Control+Shift+Space**), and
  **Left-arrow undo** with learn-from-rejection.
- **AI integration** — bring-your-own-key, OpenAI-compatible. Groq / DeepSeek / OpenAI presets,
  a model picker with refresh, and a connection test. Default provider: Groq.
- **Clipboard history** — auto-captures text copies into a bounded history with configurable max
  entry length and entry count (defaults 10000 chars / 100 entries; `0` = unlimited).
- **Markdown phrases** — authored as Markdown, delivered as a formatted HTML paste (Ctrl+V) with a
  plain-text fallback.
- **Per-phrase hotkeys & app-scoping** — bind a phrase to a dedicated shortcut, and optionally
  restrict it to windows whose title contains a substring.
- **Import / export** — import from CSV, AutoHotKey hotstrings, or aText backups (with a preview
  wizard); export the whole library as JSON.
- **Settings** — sectioned sidebar (Features / Spellcheck / Data / Folders / Shortcuts /
  About) in a compact two-column layout, dark/light/sepia themes, launch-on-login.
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

## Portable mode (Phase 8)
By default the database lives in the OS app-data dir (`~/.local/share/com.phrasexp.app/phrasexp.db`
on Linux). To run portably (e.g. from a USB stick), create an empty marker file next to the
executable:
```bash
touch /path/to/phrasexp    # the install dir
touch /path/to/phrasexp.portable
```
When `phrasexp.portable` is present, `phrasexp.db` is read/written next to the executable instead
of the app-data dir.

## At-rest encryption (optional)
Settings → Features → **Encrypt sensitive data at rest** AES-encrypts sensitive phrases, the AI
API key, and clipboard history in the DB. The random key is generated on first enable
and held in the OS keychain (`keyring` crate); saving the toggle re-encrypts or decrypts the
library in place. When the keychain is unavailable the switch is disabled.

## Platform notes
| Platform | Silent autotext | Hotkey + popup injection | Notes |
|---|---|---|---|
| **Linux / X11** | ✅ rdev (XRecord) | ✅ enigo (XTest) | Works without extra privileges. |
| **Windows** | ✅ rdev | ✅ enigo | Works by default. |
| **macOS** | ✅ rdev | ✅ enigo | Requires **Accessibility** permission (System Settings → Privacy & Security). |
| **Linux / Wayland** | ❌ unsupported | ⚠️ via `ydotool` (uinput) | No portal for global key capture; injection needs `ydotool` + `/dev/uinput`. Hotkeys need the `ashpd` GlobalShortcuts portal. |

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
      domain/               models, pure expansion logic, errors
      db/                   rusqlite repository + migrations
      triggers/
        autotext.rs         rdev key listener + abbreviation index
        clipboard.rs        arboard poller
        inject.rs           Injector trait: enigo (X11/Win/macOS) + ydotool (Wayland)
      commands.rs           Tauri command surface (frontend ↔ Rust)
      lib.rs                app setup: settings, hotkey, tray, autostart
    capabilities/           scoped IPC permissions
    migrations/0001_init.sql
```

## Verify
```bash
cd desktop && npm run test                            # frontend unit tests (fuzzy search)
cd desktop && npm run check                           # svelte-check
cd desktop/src-tauri && cargo test                    # 113 unit tests (domain, storage, autotext)
cd desktop/src-tauri && cargo clippy --all-targets -- -D warnings   # lint-clean
cd desktop && cargo tauri dev                         # manual E2E
```
