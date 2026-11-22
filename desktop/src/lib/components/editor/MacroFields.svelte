<script lang="ts">
  import type { AiAction, PhraseSummary, Segment } from "$lib/types";
  import AiActionInlineEdit from "./AiActionInlineEdit.svelte";

  let {
    seg,
    linkable,
    previews,
    aiActions,
    aiDraftOf,
    aiDirty,
    editAi,
    saveAi,
    aiSavingId,
  }: {
    /** The segment object — edited in place ($state proxies propagate to the parent's array). */
    seg: Segment;
    /** Phrases available to link (nested macro select). */
    linkable: PhraseSummary[];
    /** path -> data URL, for previewing image segments in this editor session. */
    previews: Record<string, string>;
    /** AI action presets — the aiaction segment's select. */
    aiActions: AiAction[];
    aiDraftOf: (id: string) => { name: string; prompt_template: string } | null;
    aiDirty: (id: string) => boolean;
    editAi: (id: string, patch: Partial<{ name: string; prompt_template: string }>) => void;
    saveAi: (id: string) => Promise<void>;
    aiSavingId: string | null;
  } = $props();
</script>

{#if seg.type === "text"}
  <textarea bind:value={seg.text} placeholder="Literal text…"></textarea>
{:else if seg.macro.type === "datetime"}
  <div class="field">
    <span class="caption">strftime / chrono format</span>
    <input bind:value={seg.macro.format} />
  </div>
{:else if seg.macro.type === "datemath"}
  <div class="row" style="gap:10px">
    <div class="field" style="flex:0 0 130px">
      <span class="caption">days offset</span>
      <input type="number" bind:value={seg.macro.days_offset} />
    </div>
    <div class="field">
      <span class="caption">format</span>
      <input bind:value={seg.macro.format} />
    </div>
  </div>
{:else if seg.macro.type === "inputform"}
  <div class="row" style="gap:10px">
    <div class="field" style="flex:1">
      <span class="caption">field name (matched against expand inputs)</span>
      <input bind:value={seg.macro.field} />
    </div>
    <div class="field" style="flex:0 0 120px">
      <span class="caption">type</span>
      <select
        value={seg.macro.kind ?? "text"}
        onchange={(e) => {
          // Re-narrow inside the callback: TS discards the outer discriminant
          // narrowing in closures (seg.macro is mutable between check and call).
          const m = seg.macro;
          if (m.type === "inputform") m.kind = e.currentTarget.value as "text" | "select" | "checkbox";
        }}
      >
        <option value="text">text</option>
        <option value="select">select</option>
        <option value="checkbox">checkbox</option>
      </select>
    </div>
  </div>
  {#if (seg.macro.kind ?? "text") === "select"}
    <div class="field">
      <span class="caption">options (comma-separated)</span>
      <input
        value={(seg.macro.options ?? []).join(", ")}
        oninput={(e) => {
          const m = seg.macro;
          if (m.type === "inputform") {
            m.options = e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean);
          }
        }}
      />
    </div>
  {/if}
{:else if seg.macro.type === "random"}
  <div class="row" style="gap:10px">
    <div class="field" style="flex:0 0 110px">
      <span class="caption">min</span>
      <input type="number" bind:value={seg.macro.min} />
    </div>
    <div class="field" style="flex:0 0 110px">
      <span class="caption">max</span>
      <input type="number" bind:value={seg.macro.max} />
    </div>
  </div>
{:else if seg.macro.type === "math"}
  <div class="field">
    <span class="caption">expression (e.g. 100*1.2, round(10/3))</span>
    <input bind:value={seg.macro.expr} spellcheck="false" />
  </div>
{:else if seg.macro.type === "clipboard"}
  <div class="field">
    <span class="caption">Inserts the current clipboard text when expanded.</span>
  </div>
{:else if seg.macro.type === "nested"}
  <div class="field">
    <span class="caption">insert phrase (expands the linked phrase inline)</span>
    <select bind:value={seg.macro.phrase_id}>
      {#each linkable as pp (pp.id)}
        <option value={pp.id}>{pp.name}</option>
      {/each}
    </select>
  </div>
{:else if seg.macro.type === "env"}
  <div class="field">
    <span class="caption">environment variable name</span>
    <input bind:value={seg.macro.var} placeholder="HOME, USER, …" spellcheck="false" />
  </div>
{:else if seg.macro.type === "pick"}
  <div class="field">
    <span class="caption">options (comma-separated — one is chosen at random)</span>
    <input
      value={seg.macro.options.join(", ")}
      oninput={(e) => {
        const m = seg.macro;
        if (m.type === "pick") m.options = e.currentTarget.value.split(",").map((s) => s.trim()).filter(Boolean);
      }}
      placeholder="yes, no, maybe"
    />
  </div>
{:else if seg.macro.type === "file"}
  <div class="field">
    <span class="caption">file path (contents are inserted)</span>
    <input bind:value={seg.macro.path} placeholder="/path/to/file.txt" spellcheck="false" />
  </div>
{:else if seg.macro.type === "image"}
  <div class="field">
    <span class="caption">
      image — pasted into the target app when expanded{previews[seg.macro.path] ? "" : " (loading preview…)"}
    </span>
    {#if previews[seg.macro.path]}
      <img class="img-preview" src={previews[seg.macro.path]} alt="" />
    {/if}
  </div>
{:else if seg.macro.type === "ai"}
  <div class="field">
    <span class="caption">AI prompt (needs AI enabled + API key in Settings)</span>
    <textarea bind:value={seg.macro.prompt} placeholder="Translate to French: Hello world"></textarea>
  </div>
{:else if seg.macro.type === "aiaction"}
  {@const aid = seg.macro.action_id}
  <div class="field">
    <span class="caption">
      AI action — {"{clipboard}"} / {"{selection}"} in the prompt become the
      text at expansion.
    </span>
    <select bind:value={seg.macro.action_id}>
      {#each aiActions as a (a.id)}
        <option value={a.id}>{a.name}</option>
      {/each}
    </select>
    {#if aid}
      <AiActionInlineEdit
        draft={aiDraftOf(aid)}
        dirty={aiDirty(aid)}
        saving={aiSavingId === aid}
        onedit={(patch) => editAi(aid, patch)}
        onsave={() => saveAi(aid)}
      />
    {/if}
  </div>
{:else if seg.macro.type === "uuid"}
  <div class="field">
    <span class="caption">Inserts a fresh random UUID v4 on every expansion.</span>
  </div>
{:else if seg.macro.type === "shell"}
  <div class="field">
    <span class="caption">command (its output is inserted — only add commands you trust)</span>
    <input bind:value={seg.macro.command} placeholder="date +%d-%m-%Y" spellcheck="false" />
  </div>
{:else if seg.macro.type === "counter"}
  <div class="field">
    <span class="caption">counter name (each expansion inserts the next number — 1, 2, 3…)</span>
    <input bind:value={seg.macro.name} placeholder="invoice" spellcheck="false" />
  </div>
{:else if seg.macro.type === "delay"}
  <div class="field">
    <span class="caption">pause in milliseconds before the next segment (0–5000 — for slow apps)</span>
    <input type="number" min="0" max="5000" step="100" bind:value={seg.macro.ms} />
  </div>
{:else if seg.macro.type === "cite"}
  <div class="row" style="gap:10px">
    <div class="field" style="flex:1">
      <span class="caption">source — DOI, URL, or title (blank = asks when the phrase expands)</span>
      <input
        bind:value={seg.macro.source}
        placeholder="10.5555/3295222 or https://doi.org/… or Attention is all you need"
        spellcheck="false"
      />
    </div>
    <div class="field" style="flex:0 0 120px">
      <span class="caption">style</span>
      <select
        value={seg.macro.style ?? "apa"}
        onchange={(e) => {
          const m = seg.macro;
          if (m.type === "cite") m.style = e.currentTarget.value as "apa" | "mla" | "chicago";
        }}
      >
        <option value="apa">APA</option>
        <option value="mla">MLA</option>
        <option value="chicago">Chicago</option>
      </select>
    </div>
  </div>
{:else if seg.macro.type === "cursor"}
  <div class="field">
    <span class="caption">
      after expansion, the caret moves back here — put fill-in blanks AFTER this
      segment (approximate on Markdown/paste phrases, exact on plain text)
    </span>
  </div>
{/if}

<style>
  .caption {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    font-weight: 500;
  }
  .img-preview {
    max-width: 100%;
    max-height: 140px;
    align-self: flex-start;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
</style>
