// Settings store — values loaded once, write-through on save. SettingsPage edits a
// local draft; `save` persists and applies side-effects (autostart etc.) Rust-side.

import { api, errMsg } from "$lib/invoke";
import { toast } from "./toast.svelte";
import { statusStore } from "./status.svelte";
import type { Settings } from "$lib/types";

export const settingsStore = $state({
  values: null as Settings | null,
  loaded: false,

  async load() {
    this.values = await api.getSettings();
    this.loaded = true;
  },

  async save(payload: Settings): Promise<boolean> {
    try {
      this.values = await api.setSettings(payload);
      toast("Settings saved", "ok");
      // Listener-side settings may have flipped — refresh status dots promptly.
      void statusStore.poll();
      return true;
    } catch (e) {
      toast(errMsg(e), "err");
      return false;
    }
  },
});
