<script lang="ts">
  // Search mode — input + filtered phrase list. `filtered` and `selected` stay in
  // the orchestrator (keyboard nav needs them); this is pure presentation. Owns the
  // input ref; the orchestrator calls focus() after resets and mode switches.
  import type { PhraseSummary } from "$lib/types";

  let {
    q = $bindable(""),
    filtered,
    selected = $bindable(0),
    onpick,
  }: {
    q?: string;
    filtered: PhraseSummary[];
    selected?: number;
    onpick: (p: PhraseSummary) => void;
  } = $props();

  let inputEl: HTMLInputElement | undefined = $state();

  export function focus() {
    inputEl?.focus();
    inputEl?.select();
  }
</script>

<div class="head row between" data-tauri-drag-region>
  <span class="caption" data-tauri-drag-region>PhraseXp</span>
  <span class="caption count">{filtered.length} phrase{filtered.length === 1 ? "" : "s"}</span>
</div>
<input
  bind:this={inputEl}
  bind:value={q}
  placeholder="Search phrases…  (↑↓ navigate · Enter types · Esc dismisses)"
  aria-label="Search phrases"
/>
<div class="list scroll">
  {#each filtered as p, i (p.id)}
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
    <div class="empty faint">No phrases found.</div>
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
