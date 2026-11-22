<script lang="ts">
  // PhraseList — searchable, sortable phrase rows with roving-tabindex keyboard
  // navigation (↑↓ move selection, Enter opens, Delete removes).
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { api, errMsg } from "$lib/invoke";
  import { rankPhrases } from "$lib/fuzzy";
  import { toast } from "$lib/stores/toast.svelte";
  import type { PhraseSummary } from "$lib/types";
  import Skeleton from "../common/Skeleton.svelte";
  import EmptyState from "../common/EmptyState.svelte";
  import Icon from "../common/Icon.svelte";

  let { onedit }: { onedit: (p: PhraseSummary) => void } = $props();

  let listEl: HTMLElement | undefined = $state();
  let activeIndex = $state(-1);

  const enabledFolderIds = $derived(
    new Set(foldersStore.items.filter((f) => f.enabled).map((f) => f.id)),
  );
  // Server-side query filters; when searching locally we re-rank the results
  // fuzzily (subsequence, abbreviation-first) on top of the server's filter.
  const basePhrases = $derived(
    phrasesStore.selectedFolderId
      ? phrasesStore.items
      : phrasesStore.items.filter((p) => enabledFolderIds.has(p.folder_id)),
  );
  const visiblePhrases = $derived(
    phrasesStore.query.trim() ? rankPhrases(phrasesStore.query, basePhrases) : basePhrases,
  );

  // Reset selection when the list itself changes identity.
  $effect(() => {
    void visiblePhrases;
    activeIndex = -1;
  });

  async function copyPhrase(p: PhraseSummary) {
    try {
      await api.injectPhrase(p.id, {}, "copy");
      toast(`Copied "${p.name}" to clipboard`, "ok");
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  async function loadHistory(p: PhraseSummary) {
    try {
      app.versions = await api.listVersions(p.id);
      app.historyPhrase = p;
      app.modal = "history";
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  function onListKey(e: KeyboardEvent) {
    if (visiblePhrases.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      activeIndex = Math.min(activeIndex + 1, visiblePhrases.length - 1);
      focusRow(activeIndex);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      activeIndex = activeIndex <= 0 ? 0 : activeIndex - 1;
      focusRow(activeIndex);
    } else if (e.key === "Enter" && activeIndex >= 0) {
      e.preventDefault();
      onedit(visiblePhrases[activeIndex]);
    } else if (e.key === "Delete" && activeIndex >= 0) {
      e.preventDefault();
      void phrasesStore.remove(visiblePhrases[activeIndex]);
    }
  }

  function focusRow(i: number) {
    listEl
      ?.querySelectorAll<HTMLElement>(".phrase")
      [i]?.focus();
  }
</script>

<div class="list-wrap">
  <div class="toolbar">
    <div class="search">
      <Icon name="search" size={13} />
      <input
        bind:value={phrasesStore.query}
        oninput={() => void phrasesStore.load()}
        placeholder="Search phrases…"
        aria-label="Search phrases"
      />
    </div>
    <select bind:value={phrasesStore.sort} onchange={() => void phrasesStore.load()} aria-label="Sort phrases">
      <option value="name">Name</option>
      <option value="updated">Recently edited</option>
      <option value="created">Newest first</option>
      <option value="most_used">Most used</option>
    </select>
    <button class="primary" onclick={() => {
      const folderId = foldersStore.targetId();
      if (!folderId) return;
      app.editing = { phrase: null, folderId };
    }}>+ New phrase</button>
  </div>

  <div class="list scroll" bind:this={listEl} onkeydown={onListKey} role="listbox" tabindex="-1" aria-label="Phrases">
    {#if phrasesStore.loading && visiblePhrases.length === 0}
      <div class="pad"><Skeleton rows={5} height={54} /></div>
    {:else if visiblePhrases.length === 0}
      <EmptyState
        icon="library"
        title="No phrases here yet"
        hint={phrasesStore.selectedFolderId
          ? "This folder is empty — create the first phrase with “+ New phrase”."
          : "Muted folders are hidden here. Enable them in Settings → Folders, or create a phrase."}
        actionLabel="+ New phrase"
        onaction={() => {
          const folderId = foldersStore.targetId();
          if (folderId) app.editing = { phrase: null, folderId };
        }}
      />
    {:else}
      {#each visiblePhrases as p, i (p.id)}
        <div
          class="phrase {activeIndex === i ? 'active' : ''}"
          role="option"
          aria-selected={activeIndex === i}
          tabindex={activeIndex === i ? 0 : -1}
          onclick={() => onedit(p)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), onedit(p))}
        >
          <div class="row between">
            <strong class="pname">{p.name}</strong>
            <div class="row" style="gap:6px">
              {#if p.expanded_count}<span class="tag" title={`used ${p.expanded_count} time${p.expanded_count === 1 ? "" : "s"}`}>×{p.expanded_count}</span>{/if}
              {#if p.abbreviation}<span class="tag mono">{p.abbreviation}</span>{/if}
              {#if p.sensitive}<span class="tag warn">sensitive</span>{/if}
            </div>
          </div>
          <div class="preview muted">{p.content_preview}</div>
          {#if p.tags?.length}
            <div class="row" style="gap:4px; flex-wrap:wrap">
              {#each p.tags as t (t)}
                <span class="tag tagchip">#{t}</span>
              {/each}
            </div>
          {/if}
          <div class="row phrase-actions">
            <button class="tiny ghost" onclick={(e) => { e.stopPropagation(); copyPhrase(p); }}>Copy</button>
            <button class="tiny ghost" onclick={(e) => { e.stopPropagation(); loadHistory(p); }}>History</button>
            <button class="tiny danger ghost" onclick={(e) => { e.stopPropagation(); void phrasesStore.remove(p); }}>Delete</button>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .tagchip {
    color: var(--accent);
    border-color: var(--accent-soft);
    font-size: var(--fs-xs);
  }
  .list-wrap {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    gap: 8px;
    padding: 10px 14px;
    align-items: center;
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-faint);
  }
  .search input {
    border: none;
    background: transparent;
    padding: 7px 0;
    box-shadow: none;
  }
  .search input:focus {
    box-shadow: none;
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: var(--shadow-focus);
  }
  .toolbar select {
    width: auto;
    flex: 0 0 auto;
  }
  .toolbar .primary {
    flex: 0 0 auto;
  }
  .list {
    flex: 1;
    padding: 0 14px 14px;
  }
  .pad {
    padding: 8px 0;
  }
  .phrase {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 9px 12px 9px 14px;
    margin-bottom: 6px;
    position: relative;
    cursor: pointer;
    transition: border-color var(--t-fast), box-shadow var(--t-fast), transform var(--t-fast);
  }
  .phrase::before {
    content: "";
    position: absolute;
    left: 4px;
    top: 50%;
    height: 0;
    width: 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    transform: translateY(-50%);
    transition: height var(--t-fast);
  }
  .phrase:hover,
  .phrase.active {
    border-color: var(--accent);
    box-shadow: var(--shadow-sm);
    transform: translateY(-1px);
  }
  .phrase:hover::before,
  .phrase.active::before {
    height: 60%;
  }
  .phrase:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .phrase-actions {
    justify-content: flex-end;
    margin-top: 6px;
    opacity: 0.4;
    transition: opacity var(--t-fast);
  }
  .phrase:hover .phrase-actions,
  .phrase:focus-within .phrase-actions,
  .phrase.active .phrase-actions {
    opacity: 1;
  }
  .pname {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .preview {
    margin-top: 2px;
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
