<script lang="ts">
  // Folders panel — create / mute / delete, using the shared folders store.
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { toast } from "$lib/stores/toast.svelte";

  let newFolderName = $state("");
  let folderErr = $state<Record<string, string>>({});

  async function createFolder() {
    const name = newFolderName.trim();
    if (!name) return;
    const f = await foldersStore.create(name);
    if (f) newFolderName = "";
  }

  async function toggleFolder(id: string, enabled: boolean) {
    await foldersStore.setEnabled(id, enabled);
  }

  async function removeFolder(f: { id: string; name: string }) {
    if (!confirm(`Delete folder "${f.name}"? It must be empty — merge or delete its snippets first.`)) return;
    const ok = await foldersStore.remove(f.id);
    if (!ok) {
      const count = foldersStore.counts.get(f.id) ?? 0;
      folderErr = {
        ...folderErr,
        [f.id]: count > 0 ? `Has ${count} snippet${count === 1 ? "" : "s"} — merge or delete them first.` : "Delete failed",
      };
    }
  }

  // Merge flow: pick a target folder for the row's folder, confirm, bulk-move.
  let merging = $state<{ id: string; name: string } | null>(null);

  async function mergeInto(target: { id: string; name: string }) {
    if (!merging) return;
    const src = merging;
    const count = foldersStore.counts.get(src.id) ?? 0;
    if (!confirm(`Move ${count} snippet${count === 1 ? "" : "s"} from "${src.name}" into "${target.name}" and delete "${src.name}"?`)) {
      return;
    }
    const moved = await foldersStore.merge(src.id, target.id);
    if (moved !== null) {
      toast(`Moved ${moved} snippet${moved === 1 ? "" : "s"} into "${target.name}"`, "ok");
      merging = null;
    }
  }
</script>

<div class="col">
  <section class="card">
    <div class="card-h">New folder</div>
    <div class="row" style="gap:8px">
      <input bind:value={newFolderName} placeholder="Folder name…" style="flex:1"
        onkeydown={(e) => e.key === "Enter" && createFolder()} />
      <button class="primary" onclick={createFolder} disabled={!newFolderName.trim()}>+ Add</button>
    </div>
  </section>

  <section class="card">
    <div class="card-h">Your folders</div>
    {#if foldersStore.items.length === 0}
      <p class="faint" style="margin:0">No folders yet. Folders group your snippets and let you mute a group from triggering.</p>
    {:else}
      <div class="col" style="gap:6px">
        {#each foldersStore.items as f (f.id)}
          <div class="folder-row" class:off={!f.enabled}>
            <span class="folder-ico">📁</span>
            <div class="folder-meta">
              <span class="folder-name">{f.name}</span>
              {#if folderErr[f.id]}
                <span class="error folder-err">{folderErr[f.id]}</span>
              {:else}
                <span class="faint folder-count">
                  {foldersStore.counts.get(f.id) ?? 0} snippet{(foldersStore.counts.get(f.id) ?? 0) === 1 ? "" : "s"}{f.enabled ? "" : " · muted"}
                </span>
              {/if}
            </div>
            <label class="switch" title={f.enabled ? "Disable (mute snippets)" : "Enable"}>
              <input type="checkbox" checked={f.enabled ?? true}
                onchange={(e) => toggleFolder(f.id, e.currentTarget.checked)} />
              <span class="slider"></span>
            </label>
            <button class="tiny ghost" title="Merge into another folder" onclick={() => (merging = merging?.id === f.id ? null : f)}>⇄</button>
            <button class="tiny danger ghost" title="Delete folder" onclick={() => removeFolder(f)}>🗑</button>
          </div>
          {#if merging?.id === f.id}
            <div class="merge-picker">
              <span class="faint">Merge “{f.name}” into:</span>
              {#each foldersStore.items as t (t.id)}
                {#if t.id !== f.id}
                  <button class="tiny" onclick={() => mergeInto(t)}>{t.name}</button>
                {/if}
              {/each}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
    <p class="faint">Deleting requires an empty folder — ⇄ merge moves its snippets into another folder first. Toggle the switch to mute without deleting.</p>
  </section>
</div>

<style>
  .col {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
  .card-h {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: var(--fs-md);
  }
  .folder-row {
    display: grid;
    grid-template-columns: auto 1fr auto auto auto;
    align-items: center;
    gap: 10px;
    padding: 5px 9px;
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    transition: opacity var(--t-base);
  }
  .merge-picker {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 9px;
    margin-top: 4px;
    border-top: 1px dashed var(--border-soft);
  }
  .folder-row.off { opacity: 0.6; }
  .folder-ico { font-size: 16px; line-height: 1; }
  .folder-meta { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .folder-name { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .folder-count,
  .folder-err { font-size: var(--fs-xs); }
</style>
