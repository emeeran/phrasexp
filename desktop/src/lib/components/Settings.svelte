<script lang="ts">
  import { onMount } from "svelte";
  import { api, errMsg } from "$lib/invoke";
  import type { Settings } from "$lib/types";

  let s = $state<Settings | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let savedFlash = $state(false);
  let csvInput: HTMLInputElement | undefined = $state();
  let ahkInput: HTMLInputElement | undefined = $state();

  onMount(async () => {
    try {
      s = await api.getSettings();
    } catch (e) {
      error = errMsg(e);
    }
  });

  async function save() {
    if (!s) return;
    saving = true;
    error = null;
    try {
      s = await api.setSettings(s);
      savedFlash = true;
      setTimeout(() => (savedFlash = false), 1500);
    } catch (e) {
      error = errMsg(e);
    } finally {
      saving = false;
    }
  }

  async function exportLib() {
    try {
      const data = await api.exportLibrary();
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "phrasexp-library.json";
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) { error = errMsg(e); }
  }

  async function onCsvPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      const n = await api.importCsv(await file.text(), null);
      error = `Imported ${n} phrases from CSV`;
    } catch (err) { error = errMsg(err); }
    input.value = "";
  }

  async function onAhkPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      const n = await api.importAhk(await file.text(), null);
      error = `Imported ${n} phrases from AHK`;
    } catch (err) { error = errMsg(err); }
    input.value = "";
  }

  async function doReset() {
    if (!confirm("Delete ALL phrases, folders, clipboard, and version history? This cannot be undone.")) return;
    try {
      await api.resetDatabase();
      error = "Database reset — restart PhraseXp.";
    } catch (e) { error = errMsg(e); }
  }
</script>

<div class="col" style="gap:14px">
  <h3 style="margin:0">Settings</h3>

  {#if s}
    <label class="toggle">
      <input type="checkbox" bind:checked={s.autotext_enabled} />
      <span>Silent autotext <span class="faint">— expand abbreviations as you type</span></span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.clipboard_enabled} />
      <span>Capture clipboard history <span class="faint">— auto-record text copies</span></span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.spellcheck_enabled} />
      <span>Spellcheck <span class="faint">— auto-correct typos as you type</span></span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.case_adapt_enabled} />
      <span>Case adaptation <span class="faint">— SIG → ALL CAPS, Sig → Title</span></span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.autostart} />
      <span>Launch on system login</span>
    </label>

    <div class="field">
      <label for="hk">Popup hotkey</label>
      <input id="hk" bind:value={s.popup_hotkey} placeholder="Control+Alt+Space" />
    </div>

    <div class="divider"></div>

    <strong>AI Integration</strong>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.ai_enabled} />
      <span>Enable AI macros <span class="faint">— OpenAI-compatible · BYO key</span></span>
    </label>
    {#if s.ai_enabled}
      <div class="field">
        <label for="ai-url">API base URL</label>
        <input id="ai-url" bind:value={s.ai_base_url} placeholder="https://api.openai.com/v1" />
      </div>
      <div class="field">
        <label for="ai-key">API key</label>
        <input id="ai-key" type="password" bind:value={s.ai_api_key} placeholder="sk-..." />
      </div>
      <div class="field">
        <label for="ai-model">Model</label>
        <input id="ai-model" bind:value={s.ai_model} placeholder="gpt-4o-mini" />
      </div>
    {/if}

    <div class="divider"></div>

    <strong>Data</strong>
    <div class="row" style="flex-wrap:wrap; gap:6px">
      <button class="tiny" onclick={exportLib}>⭳ Export JSON</button>
      <button class="tiny" onclick={() => csvInput?.click()}>⭱ Import CSV</button>
      <button class="tiny" onclick={() => ahkInput?.click()}>⭱ Import AHK</button>
      <button class="tiny danger" onclick={doReset}>🗑 Reset Database</button>
    </div>
    <input bind:this={csvInput} type="file" accept=".csv,text/csv" style="display:none" onchange={onCsvPicked} />
    <input bind:this={ahkInput} type="file" accept=".ahk,.txt" style="display:none" onchange={onAhkPicked} />

    {#if error}<div class="error">{error}</div>{/if}
    <div class="row">
      <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save settings"}</button>
      {#if savedFlash}<span class="tag">saved</span>{/if}
    </div>
  {:else}
    <div class="faint">Loading…</div>
  {/if}
</div>

<style>
  .toggle {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    cursor: pointer;
  }
  .toggle input {
    width: auto;
    margin-top: 3px;
  }
  .divider {
    height: 1px;
    background: var(--border-soft);
    margin: 6px 0;
  }
</style>
