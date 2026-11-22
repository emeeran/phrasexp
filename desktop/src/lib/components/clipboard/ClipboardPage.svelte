<script lang="ts">
  // Clipboard page — full-width list + preview, reusing the legacy column markup's data.
  import { clipboardStore } from "$lib/stores/clipboard.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import EmptyState from "../common/EmptyState.svelte";
  import Icon from "../common/Icon.svelte";
  import Skeleton from "../common/Skeleton.svelte";
  import { fmtDateTime } from "$lib/dates";

  let newClipText = $state("");
  let selectedId = $state<string | null>(null);
  const selected = $derived(clipboardStore.items.find((c) => c.id === selectedId) ?? clipboardStore.items[0] ?? null);

  async function addClip() {
    await clipboardStore.add(newClipText);
    newClipText = "";
  }

  function clipToPhrase(text: string) {
    const folderId = foldersStore.targetId();
    if (!folderId) return;
    app.editing = { phrase: null, folderId, initialText: text };
    app.routeTo("library");
  }

  async function copyBack(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast("Copied to clipboard", "ok");
    } catch {
      toast("Clipboard unavailable", "err");
    }
  }
</script>

<div class="page">
  <div class="head">
    <h2>Clipboard</h2>
    <div class="row">
      <input bind:value={newClipText} placeholder="Capture text…" onkeydown={(e) => e.key === "Enter" && addClip()} />
      <button onclick={addClip}>Add</button>
      {#if clipboardStore.items.length > 0}
        <button class="danger ghost" onclick={() => clipboardStore.clear()}>Clear all</button>
      {/if}
    </div>
  </div>

  <div class="body">
    <div class="list scroll">
      {#if !clipboardStore.loaded}
        <Skeleton rows={6} height={44} />
      {:else if clipboardStore.items.length === 0}
        <EmptyState
          icon="clipboard"
          title="Nothing captured yet"
          hint="Copy text anywhere — PhraseXp keeps the last entries here (if capture is on in Settings → Features)."
        />
      {:else}
        {#each clipboardStore.items as c (c.id)}
          <button
            class="row-item {selected?.id === c.id ? 'on' : ''}"
            onclick={() => (selectedId = c.id)}
          >
            <span class="clip-text">{c.text}</span>
            <span class="faint time">{c.captured_at.slice(11, 19)}</span>
          </button>
        {/each}
      {/if}
    </div>
    <div class="preview">
      {#if selected}
        <div class="row between">
          <span class="muted">{fmtDateTime(selected.captured_at)}</span>
          <div class="row">
            <button class="tiny" onclick={() => copyBack(selected.text)}>Copy back</button>
            <button class="tiny" onclick={() => clipToPhrase(selected.text)}>Save as phrase</button>
          </div>
        </div>
        <pre class="text">{selected.text}</pre>
      {:else}
        <EmptyState icon="eye" title="Select an entry" />
      {/if}
    </div>
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 14px 18px 10px;
  }
  .head h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-display);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
  }
  .head input {
    width: 260px;
  }
  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 1px;
    flex: 1;
    min-height: 0;
    background: var(--border-soft);
  }
  .list {
    background: var(--surface-0);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .row-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 7px 10px;
    font-size: var(--text-dense);
    color: var(--text);
    transition: background var(--t-fast), border-color var(--t-fast);
  }
  .row-item:hover {
    background: var(--surface-1);
  }
  .row-item.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .clip-text {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .time {
    font-family: var(--font-mono);
    font-size: 10px;
  }
  .preview {
    background: var(--surface-0);
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    min-width: 0;
  }
  .text {
    flex: 1;
    margin: 0;
    padding: 12px;
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--fs-base);
    white-space: pre-wrap;
    word-break: break-word;
    overflow: auto;
  }
</style>
