<script lang="ts">
  import type { ExecuteMode } from "$lib/types";

  let {
    abbreviation = $bindable(""),
    hotkey = $bindable(""),
    isRegex = $bindable(false),
    execute = $bindable<ExecuteMode>("immediate"),
    appScope = $bindable(""),
  }: {
    abbreviation?: string;
    hotkey?: string;
    isRegex?: boolean;
    execute?: ExecuteMode;
    appScope?: string;
  } = $props();
</script>

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

{#if !isRegex && abbreviation.trim()}
  <div class="field">
    <label for="pe-exec">Expand</label>
    <select id="pe-exec" bind:value={execute}>
      <option value="immediate">Immediately — as soon as the abbreviation is typed</option>
      <option value="after_delimiter">After Space or punctuation — lets you type past it (e.g. “sig” inside “signature” won't fire)</option>
    </select>
    <span class="faint" style="font-size:var(--fs-xs)">Delimiters: Space, Enter, Tab, , . ; : ! ?</span>
  </div>
{/if}

<div class="field">
  <label for="pe-app">App scope (optional)</label>
  <input id="pe-app" bind:value={appScope} placeholder="e.g. Firefox — phrase only active when the focused window title contains this (blank = everywhere)" />
</div>
