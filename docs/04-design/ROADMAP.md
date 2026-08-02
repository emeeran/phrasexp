# phrasexp — Feature Roadmap (phased)

> Companion to `DOMAIN.md` (§5 Deferred Register) and ADR 0001.
> Effort: **S** hours · **M** a day or two · **L** multi-day · **XL** weeks / needs architecture.
> Legend: ✅ done · 🔜 next · ⏳ later · 🪨 big rock (needs a product/architecture decision)

---

## Already shipped (v1 + polish)
Phrase library (folders, CRUD, search, soft-delete, sort) · autotext (case-adaptation, X11/macOS/Win
via rdev/enigo) · hotkeys + cursor popup · macros: datetime, datemath, inputform(text), random,
math, clipboard · clipboard history (auto-capture) · settings + tray + autostart + configurable
hotkey · quick-add · CSV import · JSON export · copy-output mode · sensitive masking ·
cross-platform desktop (Win/macOS/Linux-X11).

---

## Phase 1 — Core-expander polish 🔜 (fits current architecture; no big decisions)
- **Nested / linked phrases** — a macro that inserts another phrase (reuse without duplication). **M**
- **Duplicate finder** — flag phrases with identical content. **S**
- **Multi-match selection menu** — when a trigger is ambiguous, show a chooser instead of first-match. **M**
- **Version history / restore** — keep prior phrase bodies, restore on demand. **M**
- **Advanced input types** — dropdown / checkbox / radio in input forms. **M**

## Phase 2 — Triggers & context
- **Per-application phrase sets** — different phrases/triggers depending on the focused app. **M**
- **App-window & clipboard-content triggers** — fire on window change or clipboard match. **M**
- **Time / date trigger** — fire at a scheduled time. **S**
- **Regex autotext** — match abbreviations by pattern. **M**
- **Per-phrase hotkeys** — bind a phrase directly to a global shortcut. **S**

## Phase 3 — Content & macros
- **RichText / HTML bodies** + best-effort formatted paste (HTML/Markdown). **L** (formatted injection is fundamentally limited cross-app)
- **External-data macros** — environment variables, file contents, shell-command output. **M**
- **Phrase randomization** — pick one expansion randomly from a set. **S**
- **Markdown support** for phrase bodies. **M**

## Phase 4 — Automation engine 🪨
- **Program launch, app-focus, raw keystroke blocks.** **M**
- **Conditionals + loops** (if/then/else, repeat) in a safe expression language. **L**
- **Message boxes, delays, string/math ops** as macros. **M**
- **Macro recorder** — record and replay input sequences. **L**

## Phase 5 — Autocomplete & spelling 🪨
- **Inline autocomplete** — suggest words/sentences, learning from usage. **L**
- **Spelling correction** — auto-learn personal typos + multi-language dictionary. **L**

## Phase 6 — Sync & collaboration 🪨 (requires a backend service)
- **Cloud sync** — Dropbox / Google Drive / OneDrive / self-hosted. **XL**
- **Conflict-free merging** for multi-device/multi-user edits. **XL**
- **Shared / team libraries** — client-server with personal folders. **XL**
- **Identity & SSO** (e.g. Azure Entra). **XL**
- **Usage statistics.** **M**

## Phase 7 — AI integration 🪨
- **Provider-configurable LLM gateway** (BYO API key). **L**
- **Translate / rewrite / shorten / expand** macros. **M**
- **Embedded AI-generated content** in static phrases. **M**

## Phase 8 — Platform, packaging, security
- **macOS** Accessibility UX + notarization. **M**
- **Windows** code signing (AV trust for the global key listener). **M**
- **Wayland parity** — GlobalShortcuts portal hotkeys + `ydotool` injection. **L**
- **Portable / USB mode.** **S**
- **AES-encrypted phrase database** + per-phrase/link encryption. **M**

---

## Cross-cutting (every phase)
Unit + integration tests · `cargo clippy -D warnings` clean · frontend a11y · per-OS `cargo tauri
build` verification · keep `DOMAIN.md` / `SPEC.md` / this roadmap in sync.

## Suggested order
1. **Finish Phase 1** (it's in flight and all low-risk).
2. **Phase 2 → 3** (triggers + content) — the biggest "feel" gains for the least risk.
3. Then decide which 🪨 big rock to commit to: **automation (4)**, **autocomplete/spelling (5)**,
   **sync/team (6)**, or **AI (7)** — each is a multi-phase effort of its own.
4. **Phase 8** hardening in parallel with whatever big rock is chosen, before any public release.

## Open decisions (resolve before starting the big rocks)
- Do we ship a backend at all? (gates Phase 6 entirely; affects scope massively)
- Which LLM provider(s) / bring-your-own-key? (Phase 7)
- Is formatted (RichText) delivery required, given it's unreliable cross-app? (Phase 3)
- Target platforms for v1 release: Windows-only, or Win+macOS+Linux? (Phase 8)
