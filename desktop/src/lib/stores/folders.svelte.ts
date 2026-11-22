// Folders store — list + counts + CRUD (create/enable/delete/merge live in Settings,
// but the tree is shared by Library's FolderTree and QuickAdd).

import { api, errMsg } from "$lib/invoke";
import { toast } from "./toast.svelte";
import { phrasesStore } from "./phrases.svelte";
import type { Folder } from "$lib/types";

export const foldersStore = $state({
  items: [] as Folder[],
  counts: new Map<string, number>(),
  loaded: false,

  async load() {
    this.items = await api.listFolders();
    const counts = await api.folderCounts();
    this.counts = new Map(counts.map(([id, n]) => [id, n] as [string, number]));
    this.loaded = true;
  },

  // Where a new phrase goes: the selected folder, else the first one. Returns null
  // (with a warning toast) when the library has no folders at all.
  targetId(): string | null {
    const id = phrasesStore.selectedFolderId ?? this.items[0]?.id;
    if (!id) toast("Create a folder first.", "warn");
    return id ?? null;
  },

  async create(name: string, parentId: string | null = null): Promise<Folder | null> {
    try {
      const f = await api.createFolder({ name, parent_id: parentId });
      await this.load();
      toast(`Folder “${name}” created`, "ok");
      return f;
    } catch (e) {
      toast(errMsg(e), "err");
      return null;
    }
  },

  async setEnabled(id: string, enabled: boolean) {
    try {
      await api.setFolderEnabled(id, enabled);
      await this.load();
    } catch (e) {
      toast(errMsg(e), "err");
    }
  },

  async remove(id: string): Promise<boolean> {
    try {
      await api.deleteFolder(id);
      await this.load();
      return true;
    } catch (e) {
      toast(errMsg(e), "err");
      return false;
    }
  },

  /** Merge source into target (bulk phrase move + source delete). Returns the
   *  number of phrases moved, or null on failure. Refreshes phrases too —
   *  their folder assignments changed behind the phrase store's back. */
  async merge(sourceId: string, targetId: string): Promise<number | null> {
    try {
      const moved = await api.mergeFolders(sourceId, targetId);
      await Promise.all([this.load(), phrasesStore.load()]);
      // If the merged-away folder was selected, fall back to "all phrases".
      if (phrasesStore.selectedFolderId === sourceId) phrasesStore.selectFolder(null);
      return moved;
    } catch (e) {
      toast(errMsg(e), "err");
      return null;
    }
  },
});
