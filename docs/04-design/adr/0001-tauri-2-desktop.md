# ADR 0001 — Desktop platform: Tauri 2 (Rust + Svelte 5), with Linux X11 + Wayland input

- **Status:** Accepted
- **Date:** 2026-08-01
- **Discharges:** `raw_idea.txt` "Tech: provide three best options" (deferred from `DOMAIN.md §10`)
- **Supersedes:** the v1 "HTTP-delivery" assumption in `SPEC.md` (C15 host port now in-scope)

## Context
A text expander's defining capability — detect a trigger anywhere and **inject the expansion into
the focused app** — requires OS-level access (global hotkeys, keyboard hooks, synthetic input,
clipboard). The existing FastAPI backend can only *return* text over HTTP; it cannot perform any of
these. We need a desktop shell that can, while keeping the existing domain model and SQLite schema.

## Decision
Adopt **Tauri 2**: a Rust core owns storage + domain logic + OS integration; a **Svelte 5** webview
(SvelteKit in static-SPA mode, `ssr=false`) is the UI. The FastAPI code is retained in-repo as the
API/spec reference and is **not shipped** in the desktop binary. The SQLite schema is shared, so a
`phrasexp.db` works in either app.

Reference implementation: **[Espanso](https://github.com/espanso/espanso)** (mature Rust text expander).

### Alternatives considered
| Option | Verdict |
|---|---|
| **Electron** | Rejected — ships a full Chromium + Node runtime (~100 MB+), heavy RAM; input injection still needs native modules. |
| **Qt / Flutter (native)** | Rejected — different UI stack (less web reuse), steeper for the Svelte/web skillset; no clear win over Tauri. |
| **FastAPI sidecar (PyInstaller)** | Rejected — bundles a Python runtime (heavy, fragile), two runtimes, and Python still can't do OS injection. |
| **Tauri 2 (chosen)** | System webview (small binary), Rust gives direct OS access (rdev/enigo/ydotool), reuses web/Svelte skills, strong security model (capabilities). |

## Linux input strategy — X11 **and** Wayland (Ubuntu)
Wayland's security model **forbids** arbitrary synthetic input and global key capture — there is no
portal for them. So the input stack is **session-aware**, selected at runtime via
`XDG_SESSION_TYPE` / `WAYLAND_DISPLAY`, behind a Rust `Injector` trait:

| Capability | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| **Inject text / backspace** | `enigo` | `enigo` | `enigo` | **`ydotool` (uinput)** |
| **Global hotkeys** | `tauri-plugin-global-shortcut` | same | same | **`ashpd` GlobalShortcuts portal** |
| **Silent autotext (key listen)** | `rdev` | `rdev`* | `rdev` | **unsupported** (see below) |

\* macOS requires **Accessibility** permission for both rdev-listen and enigo-inject.

- **Injection on Wayland** uses [`ydotool`](https://github.com/ReimuNotMoe/ydotool), which drives a
  kernel `uinput` virtual keyboard — it works under Wayland *and* X11 because it sits below the
  compositor. Requires `ydotool` installed, the `ydotoole` daemon running, and `/dev/uinput` access
  (user in the `input` group + a udev rule, or a one-off `chmod`). The app detects its absence and
  surfaces a Settings warning; injection no-ops until resolved.
- **Global hotkeys on Wayland** use the `org.freedesktop.portal.GlobalShortcuts` interface via the
  [`ashpd`](https://crates.io/crates/ashpd) crate (supported by GNOME 43+, KDE; the user grants the
  app per-shortcut via a portal dialog). If the portal is unavailable, v1 falls back to the tray
  icon + an always-available popup window (no system-wide hotkey).
- **Silent autotext on Wayland is intentionally unsupported.** There is no sanctioned way to read
  global keystrokes; the only route is raw `/dev/input/event*` (requires root or the `input` group,
  is fragile across devices, and is unsafe to ship). X11, Windows, and macOS keep full autotext via
  `rdev`. The Settings UI makes this session-dependent gap explicit.

### Trust model (carries NFR-9/10)
The autotext listener is a global keylogger *by construction*. It matches **only registered
abbreviations** and **never logs, stores, or transmits** raw keystrokes. This is stated plainly in
Settings and the README. Antivirus software on Windows may flag the global listener; code-signing
mitigates this.

## Consequences
- New runtime deps on Linux Wayland: `ydotool` + uinput permissions; `xdg-desktop-portal` +
  GlobalShortcuts-capable compositor for hotkeys.
- macOS Accessibility is a hard gate for both autotext and injection — the app must detect a missing
  grant and guide the user to grant it.
- Dev/testing on this X11 host exercises the X11 path live; the Wayland path is built here but must
  be verified on a Wayland session.
- `SPEC.md` gets an addendum: transport changes HTTP→Tauri commands; C15 host port in-scope; new
  FRs for hotkey/autotext/inject/clipboard-history; the Wayland autotext gap is a documented
  platform limitation, not a defect.
