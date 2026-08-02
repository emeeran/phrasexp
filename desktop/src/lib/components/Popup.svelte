<script lang="ts">
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { api, errMsg } from "$lib/invoke";
  import type { InputKind, Phrase, PhraseSummary } from "$lib/types";

  const win = getCurrentWindow();

  let phrases = $state<PhraseSummary[]>([]);
  let q = $state("");
  let selected = $state(0);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let inputEl: HTMLInputElement | undefined = $state();

  // search | inputs (filling input-form fields) | choose (multi-match picker)
  let mode = $state<"search" | "inputs" | "choose">("search");
  let current = $state<Phrase | null>(null);
  let formInputs = $state<Record<string, string>>({});
  let candidates = $state<PhraseSummary[]>([]);
  let chooseSelected = $state(0);

  const filtered = $derived(
    q.trim()
      ? phrases.filter(
          (p) =>
            p.name.toLowerCase().includes(q.trim().toLowerCase()) ||
            (p.abbreviation ?? "").toLowerCase().includes(q.trim().toLowerCase()),
        )
      : phrases,
  );

  function fieldsOf(p: Phrase): { field: string; kind: InputKind; options: string[] }[] {
    const out: { field: string; kind: InputKind; options: string[] }[] = [];
    for (const seg of p.body) {
      if (seg.type === "macro" && seg.macro.type === "inputform") {
        out.push({
          field: seg.macro.field,
          kind: (seg.macro.kind ?? "text") as InputKind,
          options: seg.macro.options ?? [],
        });
      }
    }
    return out;
  }

  function defaultsFor(fields: { field: string; kind: InputKind; options: string[] }[]) {
    const d: Record<string, string> = {};
    for (const f of fields) {
      d[f.field] = f.kind === "checkbox" ? "false" : f.kind === "select" ? f.options[0] ?? "" : "";
    }
    return d;
  }

  async function load() {
    try {
      phrases = await api.listPhrases();
      selected = 0;
    } catch (e) {
      error = errMsg(e);
    }
  }

  async function injectById(id: string, inputs: Record<string, string> = {}) {
    // Hide first so focus returns to the target app, then inject after a brief settle.
    await win.hide();
    await new Promise((r) => setTimeout(r, 120));
    await api.injectPhrase(id, inputs);
  }

  async function pick(p: PhraseSummary) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const full = await api.getPhrase(p.id);
      const fields = fieldsOf(full);
      if (fields.length === 0) {
        await injectById(p.id);
      } else {
        current = full;
        formInputs = defaultsFor(fields);
        mode = "inputs";
        await tick();
      }
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
    }
  }

  async function confirmInputs() {
    if (!current || busy) return;
    busy = true;
    error = null;
    try {
      await injectById(current.id, formInputs);
    } catch (e) {
      error = errMsg(e);
    } finally {
      busy = false;
    }
  }

  async function onChoose(ids: string[]) {
    const all = await api.listPhrases();
    candidates = all.filter((p) => ids.includes(p.id));
    chooseSelected = 0;
    mode = "choose";
    await tick();
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
      if (mode === "inputs" || mode === "choose") {
        mode = "search";
      } else {
        void win.hide();
      }
      return;
    }
    const list = mode === "choose" ? candidates : filtered;
    const sel = mode === "choose" ? chooseSelected : selected;
    const setSel = mode === "choose" ? (v: number) => (chooseSelected = v) : (v: number) => (selected = v);
    if (mode === "search" || mode === "choose") {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel(Math.min(sel + 1, Math.max(list.length - 1, 0)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel(Math.max(sel - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const p = list[sel];
        if (p) void pick(p);
      }
    }
  }

  onMount(async () => {
    await load();
    await reset();
    const unlistenFocus = await win.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void load();
        void reset();
      } else {
        mode = "search"; // reset mode when hidden
      }
    });
    const unlistenChoose = await listen<string[]>("autotext-choose", (e) => {
      void onChoose(e.payload);
    });
    return () => {
      void unlistenFocus();
      void unlistenChoose();
    };
  });
</script>

<svelte:window onkeydown={onkeydown} />

<div class="popup">
  {#if mode === "inputs" && current}
    <div class="inputs">
      <div class="row between">
        <strong>{current.name}</strong>
        <button class="ghost tiny" onclick={() => (mode = "search")}>‹ back</button>
      </div>
      {#each fieldsOf(current) as f (f.field)}
        <div class="field">
          <label for="fld-{f.field}">{f.field}</label>
          {#if f.kind === "checkbox"}
            <input
              id="fld-{f.field}"
              type="checkbox"
              style="width:auto"
              checked={formInputs[f.field] === "true"}
              onchange={(e) => (formInputs[f.field] = e.currentTarget.checked ? "true" : "false")}
            />
          {:else if f.kind === "select"}
            <select id="fld-{f.field}" bind:value={formInputs[f.field]}>
              {#each f.options as opt (opt)}
                <option value={opt}>{opt}</option>
              {/each}
            </select>
          {:else}
            <input id="fld-{f.field}" bind:value={formInputs[f.field]} />
          {/if}
        </div>
      {/each}
      <div class="row" style="justify-content:flex-end; margin-top:6px">
        <button class="primary" onclick={confirmInputs} disabled={busy}>
          {busy ? "…" : "Insert"}
        </button>
      </div>
    </div>
  {:else if mode === "choose"}
    <div class="muted" style="padding:2px 4px 6px">Multiple phrases match — pick one:</div>
    <div class="list scroll">
      {#each candidates as p, i (p.id)}
        <button
          class="item"
          class:on={i === chooseSelected}
          onclick={() => pick(p)}
          onmouseenter={() => (chooseSelected = i)}
        >
          <span class="name">{p.name}</span>
          {#if p.abbreviation}<span class="tag">{p.abbreviation}</span>{/if}
        </button>
      {:else}
        <div class="empty faint">No candidates.</div>
      {/each}
    </div>
  {:else}
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
  {/if}
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
  .inputs {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
</style>
