<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, errMsg } from "$lib/invoke";
  import type { PhraseSummary } from "$lib/types";

  const win = getCurrentWindow();

  let phrases = $state<PhraseSummary[]>([]);
  let q = $state("");
  let selected = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let inputEl: HTMLInputElement | undefined = $state();

  const filtered = $derived(
    q.trim()
      ? phrases.filter(
          (p) =>
            p.name.toLowerCase().includes(q.trim().toLowerCase()) ||
            (p.abbreviation ?? "").toLowerCase().includes(q.trim().toLowerCase()),
        )
      : phrases,
  );

  async function load() {
    try {
      phrases = await api.listPhrases();
      selected = 0;
    } catch (e) {
      error = errMsg(e);
    }
  }

  async function pick(p: PhraseSummary) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await api.injectPhrase(p.id);
      await win.hide();
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
    }
  }

  async function reset() {
    q = "";
    selected = 0;
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      void win.hide();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, Math.max(filtered.length - 1, 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const p = filtered[selected];
      if (p) void pick(p);
    }
  }

  onMount(async () => {
    await load();
    await reset();
    const unlisten = await win.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void load();
        void reset();
      }
    });
    return unlisten;
  });
</script>

<svelte:window onkeydown={onkeydown} />

<div class="popup">
  <input
    bind:this={inputEl}
    bind:value={q}
    placeholder="Search phrases…  (↑↓ navigate · Enter types · Esc dismisses)"
  />
  <div class="list scroll">
    {#each filtered as p, i (p.id)}
      <button
        class="item"
        class:on={i === selected}
        onclick={() => pick(p)}
        onmouseenter={() => (selected = i)}
      >
        <span class="name">{p.name}</span>
        {#if p.abbreviation}<span class="tag">{p.abbreviation}</span>{/if}
      </button>
    {:else}
      <div class="empty faint">No phrases found.</div>
    {/each}
  </div>
  {#if error}<div class="error">{error}</div>{/if}
</div>

<style>
  .popup {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg);
    padding: 8px;
    gap: 6px;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  .item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
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
