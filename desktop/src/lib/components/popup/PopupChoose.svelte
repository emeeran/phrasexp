<script lang="ts">
  // Choose mode — multi-match picker (several phrases share a trigger).
  import type { PhraseSummary } from "$lib/types";

  let {
    candidates,
    selected = $bindable(0),
    onpick,
  }: {
    candidates: PhraseSummary[];
    selected?: number;
    onpick: (p: PhraseSummary) => void;
  } = $props();
</script>

<div class="head row between" data-tauri-drag-region>
  <span class="caption" data-tauri-drag-region>Multiple phrases match — pick one</span>
  <span class="caption count">{candidates.length}</span>
</div>
<div class="list scroll">
  {#each candidates as p, i (p.id)}
    <button
      class="item"
      class:on={i === selected}
      onclick={() => onpick(p)}
      onmouseenter={() => (selected = i)}
    >
      <span class="name">{p.name}</span>
      {#if p.abbreviation}<span class="tag">{p.abbreviation}</span>{/if}
    </button>
  {:else}
    <div class="empty faint">No candidates.</div>
  {/each}
</div>

<style>
  .head {
    padding: 1px 2px 4px;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .count {
    color: var(--text-faint);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-height: 0;
    max-height: 340px;
  }
  .item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 7px 10px;
    width: 100%;
  }
  .item.on,
  .item:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    padding: 18px;
    text-align: center;
  }
</style>
