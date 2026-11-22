// Status store — single 2s poll for autotext listener health, driving the header
// status dots. Spellcheck on/off rides on settings.spellcheck_enabled; the
// `spellcheck-toggled` event refreshes it immediately.

import { api } from "$lib/invoke";
import { listen } from "@tauri-apps/api/event";
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
    setInterval(() => void this.poll(), 2000);
    // Spellcheck hotkey may flip it outside the UI; keep the dot honest.
    listen("spellcheck-toggled", () => void this.poll());
  },
});
