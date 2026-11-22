// Phrases store — the Library list: filters, selection, CRUD. Actions mutate then
// refresh their own domain; folder mutations from Settings invalidate via `load()`.

import { api, errMsg } from "$lib/invoke";
import { toast } from "./toast.svelte";
import { foldersStore } from "./folders.svelte";
import type { Phrase, PhraseSummary, SortKey } from "$lib/types";

export const phrasesStore = $state({
  items: [] as PhraseSummary[],
  selectedFolderId: null as string | null,
  query: "",
  sort: "name" as SortKey,
  loading: false,

  async load() {
    this.loading = true;
    try {
      // The query is NOT sent server-side: list loads the folder and the client-side
      // rankPhrases (recomputed per keystroke from `query`) does the matching — one
      // IPC per folder/sort change instead of one per keystroke typed.
      this.items = await api.listPhrases(
        this.selectedFolderId,
        undefined,
        false,
        this.sort,
      );
    } finally {
      this.loading = false;
    }
  },

  selectFolder(id: string | null) {
    this.selectedFolderId = id;
    void this.load();
  },

  async create(payload: Parameters<typeof api.createPhrase>[0]): Promise<Phrase | null> {
    try {
      const p = await api.createPhrase(payload);
      await this.load();
      void foldersStore.load(); // counts changed
      toast(`Phrase “${p.name}” saved`, "ok");
      return p;
    } catch (e) {
      toast(errMsg(e), "err");
      return null;
    }
  },

  async update(phraseId: string, payload: Parameters<typeof api.updatePhrase>[1]): Promise<Phrase | null> {
    try {
      const p = await api.updatePhrase(phraseId, payload);
      await this.load();
      toast(`Phrase “${p.name}” saved`, "ok");
      return p;
    } catch (e) {
      toast(errMsg(e), "err");
      return null;
    }
  },

  async remove(p: PhraseSummary): Promise<boolean> {
    if (!confirm(`Delete phrase "${p.name}"?`)) return false;
    try {
      await api.deletePhrase(p.id);
      await this.load();
      void foldersStore.load();
      toast(`Phrase “${p.name}” deleted`, "ok");
      return true;
    } catch (e) {
      toast(errMsg(e), "err");
      return false;
    }
  },
});
