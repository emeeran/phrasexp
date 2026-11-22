<script lang="ts">
  // PopupAi — AI-action chooser shown over the captured selection (selection-based
  // rewrite): a vertical dropdown menu, scrollable once the builtin set outgrows the
  // popup window. Presentational: navigation keys are handled by the Popup
  // orchestrator, same as search/choose mode.
  import Icon from "../common/Icon.svelte";
  import type { AiAction } from "$lib/types";

  let {
    actions,
    selected,
    preview,
    busy,
    onpick,
  }: {
    actions: AiAction[];
    selected: number;
    preview: string;
    busy: boolean;
    onpick: (a: AiAction) => void;
  } = $props();

  // Keep the highlighted row visible while arrowing past the scrolled fold.
  let menu: HTMLElement | undefined = $state();
  $effect(() => {
    void selected;
    menu?.querySelector(".action.on")?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="col" style="gap:4px; flex:1; min-height:0">
  <div class="head row between" data-tauri-drag-region>
    <span class="caption" data-tauri-drag-region>AI action on selection</span>
    <span class="preview" data-tauri-drag-region title={preview}>{preview.length > 48 ? `${preview.slice(0, 48)}…` : preview}</span>
  </div>
  <div class="menu" bind:this={menu}>
    {#each actions as a, i (a.id)}
      <button
        class="action"
        class:on={i === selected}
        disabled={busy}
        onclick={() => onpick(a)}
      >
        <Icon name="ai" size={13} />
        <span class="label">{a.name}</span>
        {#if i < 9}<span class="key">{i + 1}</span>{/if}
      </button>
    {/each}
  </div>
  {#if busy}
    <div class="caption" style="padding:4px 8px">Working…</div>
  {/if}
</div>

<style>
  .head {
    padding: 2px 8px 4px;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .preview {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .menu {
    flex: 1;
    min-height: 0;
    max-height: 380px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 2px;
  }
  .action {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-xs);
    padding: 6px 8px;
    font-size: var(--fs-base);
    color: var(--text);
  }
  .action:hover:not(:disabled),
  .action:focus-visible,
  .action.on {
    background: var(--accent-soft);
    color: var(--accent);
    outline: none;
  }
  .action:disabled {
    opacity: 0.5;
    cursor: wait;
  }
  .label {
    flex: 1;
  }
  .key {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    border: 1px solid var(--border);
    border-radius: var(--radius-xs);
    padding: 0 5px;
  }
</style>
