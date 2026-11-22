<script lang="ts">
  // FolderTree — vertical folder list (replaces the old horizontal chip bar).
  // Counts from foldersStore; disabled folders are dimmed with a mute hint.
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import Icon from "../common/Icon.svelte";

  let { oncollapse }: { oncollapse?: () => void } = $props();

  let newFolder = $state("");
  let creating = $state(false);

  async function addFolder() {
    const name = newFolder.trim();
    if (!name) return;
    const f = await foldersStore.create(name);
    newFolder = "";
    creating = false;
    if (f) phrasesStore.selectFolder(f.id);
  }
</script>

<div class="tree">
  <div class="tree-head">
    <span class="label">Folders</span>
    <div class="head-actions">
      <button
        class="ghost tiny icon-btn"
        aria-label="New folder"
        title="New folder"
        onclick={() => (creating = !creating)}
      >
        <Icon name="plus" size={13} />
      </button>
      {#if oncollapse}
        <button
          class="ghost tiny icon-btn"
          aria-label="Collapse folders panel"
          title="Collapse folders panel"
          onclick={oncollapse}
        >
          <Icon name="chevron" size={13} />
        </button>
      {/if}
    </div>
  </div>

  {#if creating}
    <div class="new-folder">
      <input
        placeholder="Folder name…"
        bind:value={newFolder}
        onkeydown={(e) => {
          if (e.key === "Enter") addFolder();
          if (e.key === "Escape") { creating = false; newFolder = ""; }
        }}
      />
    </div>
  {/if}

  <button
    class="item {phrasesStore.selectedFolderId === null ? 'on' : ''}"
    aria-current={phrasesStore.selectedFolderId === null ? "true" : undefined}
    onclick={() => phrasesStore.selectFolder(null)}
  >
    <Icon name="library" size={14} />
    <span class="name">All phrases</span>
    <span class="count">{phrasesStore.items.length}</span>
  </button>

  {#each foldersStore.items as f (f.id)}
    <button
      class="item {phrasesStore.selectedFolderId === f.id ? 'on' : ''} {!f.enabled ? 'muted' : ''}"
      aria-current={phrasesStore.selectedFolderId === f.id ? "true" : undefined}
      title={f.enabled ? f.name : `${f.name} (muted — enable in Settings)`}
      onclick={() => phrasesStore.selectFolder(f.id)}
    >
      <span class="swatch" class:off={!f.enabled}></span>
      <span class="name">{f.name}</span>
      <span class="count">{foldersStore.counts.get(f.id) ?? 0}</span>
    </button>
  {/each}

  {#if foldersStore.items.length === 0}
    <p class="empty-hint">
      No folders yet — create one above, or Settings → Folders manages them.
    </p>
  {/if}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 8px;
    height: 100%;
    overflow-y: auto;
  }
  .tree-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 2px 6px 8px;
  }
  .head-actions {
    display: flex;
    gap: 2px;
  }
  .head-actions .icon-btn :global(svg) {
    transform: rotate(180deg); /* chevron points left = "collapse" */
  }
  .label {
    font-size: var(--text-dense);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .icon-btn {
    width: 24px;
    height: 24px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
  }
  .new-folder {
    padding: 0 4px 8px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 6px 9px;
    font-size: var(--fs-base);
    color: var(--text-dim);
    transition: background var(--t-fast), color var(--t-fast), border-color var(--t-fast);
  }
  .item:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .item.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 500;
  }
  .item.muted {
    opacity: 0.55;
  }
  .item:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--accent);
    flex: 0 0 auto;
  }
  .swatch.off {
    background: var(--dot-off);
  }
  .name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
    color: var(--text-faint);
  }
  .item.on .count {
    color: var(--accent);
  }
  .empty-hint {
    margin: 8px 6px;
    font-size: var(--text-dense);
    color: var(--text-faint);
    line-height: 1.5;
  }
</style>
