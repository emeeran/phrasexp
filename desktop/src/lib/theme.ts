/** Theme management — persisted in localStorage, applied via data-theme on <html>.
 *
 * Tauri webview windows share a localStorage origin, so all windows read the same persisted
 * value on mount. A live toggle in one window broadcasts `theme-changed` so the others (notably
 * the long-lived popup window, which mounts once at startup) re-apply it and stay in sync. */

import { emit } from "@tauri-apps/api/event";

export type Theme = "dark" | "light" | "sepia";

const ORDER: Theme[] = ["dark", "light", "sepia"];

export function getTheme(): Theme {
  const saved = localStorage.getItem("phrasexp-theme");
  return ORDER.includes(saved as Theme) ? (saved as Theme) : "dark";
}

/** Apply the persisted theme to this window's document. Use on mount. */
export function applyTheme(): Theme {
  const theme = getTheme();
  document.documentElement.dataset.theme = theme;
  return theme;
}

/** Set + persist a specific theme on this window (no broadcast). */
export function setTheme(theme: Theme): Theme {
  localStorage.setItem("phrasexp-theme", theme);
  document.documentElement.dataset.theme = theme;
  return theme;
}

/** Cycle this window's theme (dark → light → sepia), persist it, and broadcast to every
 * window so they match. */
export function toggleTheme(): Theme {
  const next = ORDER[(ORDER.indexOf(getTheme()) + 1) % ORDER.length];
  setTheme(next);
  void emit("theme-changed", next);
  return next;
}
