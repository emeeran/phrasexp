<script lang="ts">
  import type { Macro, Phrase, PhraseSummary, Segment, TriggerBinding } from "$lib/types";
  import { api, errMsg } from "$lib/invoke";

  let {
    phrase = null,
    folderId,
    phrases = [],
    onsaved,
    oncancel,
  }: {
    phrase?: Phrase | null;
    folderId: string;
    phrases?: PhraseSummary[];
    onsaved: (p: Phrase) => void;
    oncancel: () => void;
  } = $props();

  /** Phrases available to link (exclude self to avoid trivial self-recursion). */
  const linkable = $derived(phrases.filter((p) => p.id !== phrase?.id));

  // svelte-ignore state_referenced_locally
  const isEdit = !!phrase;
  // Snapshot the prop once — the editor is a fresh snapshot, not a live view of `phrase`.
  // svelte-ignore state_referenced_locally
  const src = phrase;

  let name = $state(src?.name ?? "");
  let abbreviation = $state(src?.abbreviation ?? "");
  let hotkey = $state(src?.triggers.find((t) => t.type === "hotkey")?.key ?? "");
  let isRegex = $state(src?.triggers.find((t) => t.type === "autotext")?.regex ?? false);
  let sensitive = $state(src?.sensitive ?? false);
  let segments: Segment[] = $state(
    src?.body ? structuredClone($state.snapshot(src.body)) : [{ type: "text", text: "" }],
  );

  let error = $state<string | null>(null);
  let saving = $state(false);

  function addText() {
    segments.push({ type: "text", text: "" });
  }
  function addMacro(type: Macro["type"]) {
    const macro: Macro =
      type === "inputform"
        ? { type, field: "name", kind: "text", options: [] }
        : type === "datemath"
          ? { type, days_offset: 0, format: "%Y-%m-%d" }
          : type === "datetime"
            ? { type, format: "%Y-%m-%d %H:%M" }
            : type === "random"
              ? { type, min: 1, max: 100 }
              : type === "math"
                ? { type, expr: "2+2" }
                : type === "nested"
                  ? { type, phrase_id: linkable[0]?.id ?? "" }
                  : { type: "clipboard" };
    segments.push({ type: "macro", macro });
  }
  function removeAt(i: number) {
    segments.splice(i, 1);
  }
  function move(i: number, dir: number) {
    const j = i + dir;
    if (j < 0 || j >= segments.length) return;
    const tmp = segments[i];
    segments[i] = segments[j];
    segments[j] = tmp;
  }

  async function save() {
    error = null;
    if (!name.trim()) return (error = "Name is required");
    if (segments.length === 0) return (error = "Body needs at least one segment");
    const triggers: TriggerBinding[] = [];
    if (abbreviation.trim())
      triggers.push({ type: "autotext", abbreviation: abbreviation.trim(), regex: isRegex });
    if (hotkey.trim()) triggers.push({ type: "hotkey", key: hotkey.trim() });

    const body = $state.snapshot(segments) as Segment[];
    saving = true;
    try {
      const saved: Phrase = isEdit && phrase
        ? await api.updatePhrase(phrase.id, {
            name: name.trim(),
            abbreviation: abbreviation.trim() || null,
            body,
            triggers,
            sensitive,
          })
        : await api.createPhrase({
            folderId,
            name: name.trim(),
            abbreviation: abbreviation.trim() || null,
            body,
            triggers,
            sensitive,
          });
      onsaved(saved);
    } catch (e) {
      error = errMsg(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="col" style="gap:14px">
  <div class="row between">
    <h3 style="margin:0">{isEdit ? "Edit phrase" : "New phrase"}</h3>
    <span class="tag muted">plain · {folderId.slice(0, 8)}</span>
  </div>

  <div class="field">
    <label for="pe-name">Name</label>
    <input id="pe-name" bind:value={name} placeholder="Signature" />
  </div>

  <div class="row" style="gap:10px">
    <div class="field" style="flex:1">
      <label for="pe-abbr">Autotext abbreviation</label>
      <input id="pe-abbr" bind:value={abbreviation} placeholder="br," />
    </div>
    <div class="field" style="flex:1">
      <label for="pe-hot">Hotkey</label>
      <input id="pe-hot" bind:value={hotkey} placeholder="ctrl+shift+b" />
    </div>
  </div>

  <label class="row" style="gap:8px; cursor:pointer">
    <input type="checkbox" bind:checked={isRegex} style="width:auto" />
    <span class="muted">Regex abbreviation — triggers when the pattern matches the end of what you type (e.g. <code>:[a-z]+</code>)</span>
  </label>

  <label class="row" style="gap:8px; cursor:pointer">
    <input type="checkbox" bind:checked={sensitive} style="width:auto" />
    <span class="muted">Sensitive (masked in lists)</span>
  </label>

  <div class="col" style="gap:8px">
    <div class="row between">
      <strong>Body</strong>
      <div class="row">
        <button class="tiny" onclick={addText}>+ text</button>
        <button class="tiny" onclick={() => addMacro("datetime")}>+ datetime</button>
        <button class="tiny" onclick={() => addMacro("datemath")}>+ datemath</button>
        <button class="tiny" onclick={() => addMacro("inputform")}>+ input</button>
        <button class="tiny" onclick={() => addMacro("random")}>+ random</button>
        <button class="tiny" onclick={() => addMacro("math")}>+ math</button>
        <button class="tiny" onclick={() => addMacro("clipboard")}>+ clipboard</button>
        <button class="tiny" onclick={() => addMacro("nested")} disabled={linkable.length === 0}>
          + nested
        </button>
      </div>
    </div>
    {#each segments as seg, i (i)}
      <div class="segment">
        <div class="row between">
          <span class="tag">{seg.type === "text" ? "text" : `macro · ${seg.macro.type}`}</span>
          <div class="row">
            <button class="tiny ghost" onclick={() => move(i, -1)}>↑</button>
            <button class="tiny ghost" onclick={() => move(i, 1)}>↓</button>
            <button class="tiny danger" onclick={() => removeAt(i)}>✕</button>
          </div>
        </div>
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
                onchange={(e) => (seg.macro.kind = e.currentTarget.value as "text" | "select" | "checkbox")}
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
                oninput={(e) =>
                  (seg.macro.options = e.currentTarget.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean))}
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
        {/if}
      </div>
    {/each}
  </div>

  {#if error}<div class="error">{error}</div>{/if}

  <div class="row" style="justify-content:flex-end">
    <button onclick={oncancel}>Cancel</button>
    <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
  </div>
</div>

<style>
  .segment {
    background: var(--bg-elev);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .caption {
    font-size: 12px;
    color: var(--text-dim);
    font-weight: 500;
  }
</style>
