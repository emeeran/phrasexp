// Status store — single 2s poll for autotext listener health, driving the header
// status dots. Spellcheck on/off rides on settings.spellcheck_enabled; the
// `spellcheck-toggled` event refreshes it immediately. The poll pauses while the
// main window is hidden (tray) — invisible dots don't justify an IPC round trip
// every 2s; one final poll on re-focus makes them honest again.

import { api } from "$lib/invoke";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AutotextStatus } from "$lib/types";

export const statusStore = $state({
  autotext: null as AutotextStatus | null,

  async poll() {
    try {
      this.autotext = await api.autotextStatus();
    } catch {
      /* transient IPC failure — next tick retries */
    }
  },

  /** Start the shared poll + event listeners. Called once from AppShell. */
  start() {
    void this.poll();
    const win = getCurrentWindow();
    setInterval(async () => {
      if (!(await win.isVisible().catch(() => true))) return;
      void this.poll();
    }, 2000);
    // Re-focus (un-hide) polls once immediately — no 2s of stale dots.
    win.onFocusChanged(({ payload: focused }) => {
      if (focused) void this.poll();
    });
    // Spellcheck hotkey may flip it outside the UI; keep the dot honest.
    listen("spellcheck-toggled", () => void this.poll());
  },
});
