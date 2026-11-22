<script lang="ts">
  // Offer mode — spellcheck correction candidates with live diff preview.
  // Pure presentation; keys arrive via the orchestrator's rdev-routed handler.
  import type { DiffChar } from "./diff";

  let {
    original,
    candidates,
    selected = $bindable(0),
    preview,
    busy,
    onaccept,
    onignore,
  }: {
    original: string;
    candidates: string[];
    selected?: number;
    preview: { left: DiffChar[]; right: DiffChar[] };
    busy: boolean;
    onaccept: (cand: string) => void;
    onignore: () => void;
  } = $props();
</script>

<div class="offer">
  {#if candidates[selected]}
    <div class="offer-top" data-tauri-drag-region>
      <span class="offer-ico" data-tauri-drag-region>✎</span>
      <span class="orig" data-tauri-drag-region>
        {#each preview.left as pc, k (k)}
          {#if pc.type === "del"}<span class="out">{pc.ch}</span>{:else}{pc.ch}{/if}
        {/each}
      </span>
      <span class="arrow" data-tauri-drag-region>→</span>
      <span class="new" data-tauri-drag-region>
        {#each preview.right as pc, k (k)}
          {#if pc.type === "ins"}<span class="add">{pc.ch}</span>{:else}{pc.ch}{/if}
        {/each}
      </span>
    </div>
  {/if}

  <div class="list scroll">
    {#each candidates as cand, i (cand)}
      <button
        class="item offer-item"
        class:on={i === selected}
        onclick={() => onaccept(cand)}
        onmouseenter={() => (selected = i)}
      >
        <span class="idx">{i + 1}</span>
        <span class="name">{cand}</span>
        {#if i === 0}<span class="tag">best</span>{/if}
        {#if i === selected}<span class="enter">⏎</span>{/if}
      </button>
    {:else}
      <div class="empty faint">No candidates.</div>
    {/each}
  </div>

  <div class="offer-actions">
    <button class="ghost tiny" onclick={onignore} disabled={busy} title="Add to dictionary (+ or d)">
      ＋ Add “{original}” to dictionary
    </button>
    <span class="hint">↑↓ · ⏎ · Esc · ⌫ · +</span>
  </div>
</div>

<style>
  .offer {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    min-height: 0;
    animation: pop-in 0.14s ease-out both;
  }
  .offer-top {
    display: flex;
    align-items: baseline;
    gap: 7px;
    padding: 1px 2px 4px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 15px;
    line-height: 1.2;
    cursor: grab;
    user-select: none;
  }
  .offer-top:active {
    cursor: grabbing;
  }
  .offer-ico {
    color: var(--accent);
    font-size: 12px;
    align-self: center;
  }
  .offer-top .orig {
    color: var(--text-faint);
  }
  .offer-top .orig .out {
    text-decoration: line-through;
    text-decoration-color: var(--danger);
  }
  .offer-top .arrow {
    color: var(--text-faint);
    font-size: 13px;
  }
  .offer-top .new {
    color: var(--text);
    font-weight: 600;
  }
  .offer-top .new .add {
    color: var(--accent);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-height: 0;
    max-height: 220px;
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
  .offer-item {
    position: relative;
    padding: 4px 10px 4px 24px;
  }
  .offer-item .name {
    flex: 1;
    font-size: 13px;
  }
  .offer-item .idx {
    position: absolute;
    left: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: 13px;
    text-align: center;
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    color: var(--text-faint);
  }
  .offer-item.on .idx {
    color: var(--accent);
  }
  .offer-item::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    bottom: 50%;
    width: 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    transition: top 0.12s ease, bottom 0.12s ease, opacity 0.12s ease;
    opacity: 0;
  }
  .offer-item.on::before {
    top: 5px;
    bottom: 5px;
    opacity: 1;
  }
  .offer-item .tag {
    font-size: 10px;
    padding: 0 6px;
  }
  .offer-item .enter {
    color: var(--text-faint);
    font-size: 11px;
    margin-left: 3px;
  }
  .offer-item.on .enter {
    color: var(--accent);
  }
  .offer-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 1px;
  }
  .offer-actions .ghost {
    font-size: 11px;
    padding: 3px 8px;
  }
  .hint {
    font-size: 10px;
    color: var(--text-faint);
    white-space: nowrap;
  }
</style>
