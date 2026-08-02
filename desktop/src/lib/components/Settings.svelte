<script lang="ts">
  import { onMount } from "svelte";
  import { api, errMsg } from "$lib/invoke";
  import type { Settings } from "$lib/types";

  let s = $state<Settings | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let savedFlash = $state(false);

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
      <input type="checkbox" bind:checked={s.autostart} />
      <span>Launch on system login</span>
    </label>

    <div class="field">
      <label for="hk">Popup hotkey</label>
      <input id="hk" bind:value={s.popup_hotkey} placeholder="Control+Alt+Space" />
      <span class="faint">e.g. Control+Alt+Space, Shift+Alt+P, Meta+Space · keys: a–z, 0–9, Space, F1–F12</span>
    </div>

    <div class="divider" style="margin:10px 0; height:1px; background:var(--border-soft)"></div>

    <label class="toggle">
      <input type="checkbox" bind:checked={s.ai_enabled} />
      <span>Enable AI macros <span class="faint">— OpenAI-compatible · bring your own key</span></span>
    </label>
    {#if s.ai_enabled}
      <div class="field">
        <label for="ai-url">API base URL</label>
        <input id="ai-url" bind:value={s.ai_base_url} placeholder="https://api.openai.com/v1" />
      </div>
      <div class="field">
        <label for="ai-key">API key <span class="faint">(stored locally in your phrase DB)</span></label>
        <input id="ai-key" type="password" bind:value={s.ai_api_key} placeholder="sk-..." />
      </div>
      <div class="field">
        <label for="ai-model">Model</label>
        <input id="ai-model" bind:value={s.ai_model} placeholder="gpt-4o-mini" />
      </div>
    {/if}

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
</style>
