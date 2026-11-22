# Project Context: PhraseXp

**PhraseXp** is a lightweight, local-first desktop text expander (PhraseExpress-class).
The shipped product is a **Tauri 2** app: a **Rust core** (`desktop/src-tauri/`) owns storage,
triggers, and OS-level input injection; a **Svelte 5** webview (`desktop/src/`) is the UI.

## Tech Stack
- **Desktop app:** Tauri 2 — Rust (edition 2021) core + Svelte 5 / Vite frontend.
  Local SQLite (`rusqlite`), `rdev` (key listening), `enigo` (injection), `arboard` (clipboard),
  `reqwest` (AI), `pulldown-cmark` (Markdown).
- **OS:** Ubuntu 24.x (also Windows / macOS; Wayland via `ydotool`).

## Commands (desktop app)
- Dev: `cd desktop && npm install && cargo tauri dev`
- Build bundles: `cd desktop && cargo tauri build`
- Tests: `cd desktop/src-tauri && cargo test` (168 unit tests)
- Frontend tests: `cd desktop && npm run test` (vitest — fuzzy search)
- Lint: `cd desktop/src-tauri && cargo clippy -- -D warnings`
- Frontend type-check: `cd desktop && npm run check`

## Conventions
- Tauri IPC: commands (`desktop/src-tauri/src/commands.rs`) + events; no HTTP.
- Capabilities scoped in `desktop/src-tauri/capabilities/default.json`.
- See `desktop/HOW_TO_USE.md` for the user manual.
