// Clipboard store — history entries for the Clipboard page.

import { api, errMsg } from "$lib/invoke";
import { toast } from "./toast.svelte";
import type { ClipboardEntry } from "$lib/types";

export const clipboardStore = $state({
  items: [] as ClipboardEntry[],
  loaded: false,

  async load() {
    this.items = await api.listClipboard();
    this.loaded = true;
  },

  async add(text: string) {
    if (!text.trim()) return;
    try {
      await api.addClipboard(text.trim());
      await this.load();
    } catch (e) {
      toast(errMsg(e), "err");
    }
  },

  async clear() {
    try {
      await api.clearClipboard();
      await this.load();
      toast("Clipboard history cleared", "ok");
    } catch (e) {
      toast(errMsg(e), "err");
    }
  },
});
