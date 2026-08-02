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

  // AI provider presets
  const PROVIDERS: Record<string, { name: string; baseUrl: string; models: string[] }> = {
    groq: { name: "Groq", baseUrl: "https://api.groq.com/openai/v1", models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "gemma2-9b-it"] },
    deepseek: { name: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", models: ["deepseek-chat", "deepseek-reasoner"] },
    openai: { name: "OpenAI", baseUrl: "https://api.openai.com/v1", models: ["gpt-4o-mini", "gpt-4o", "gpt-3.5-turbo"] },
    custom: { name: "Custom", baseUrl: "", models: [] },
  };

  let currentProvider = $state("groq");
  let modelList = $state<string[]>([]);
  let testing = $state(false);
  let testResult = $state<string | null>(null);
  let refreshing = $state(false);

  function detectProvider(baseUrl: string): string {
    for (const [key, p] of Object.entries(PROVIDERS)) {
      if (key === "custom") continue;
      if (baseUrl.includes(p.baseUrl.replace("https://", "").split("/")[0])) return key;
    }
    return "custom";
  }

  function onProviderChange(e: Event) {
    const key = (e.target as HTMLSelectElement).value;
    currentProvider = key;
    if (s && key !== "custom") {
      s.ai_base_url = PROVIDERS[key].baseUrl;
      modelList = [...PROVIDERS[key].models];
      if (!modelList.includes(s.ai_model)) s.ai_model = modelList[0] ?? "";
    }
  }

  async function refreshModels() {
    if (!s) return;
    refreshing = true;
    testResult = null;
    try {
      // Save current settings first so the backend has the latest key/url
      s = await api.setSettings(s);
      const models = await api.listAiModels();
      modelList = models.sort();
      testResult = `Loaded ${models.length} models`;
    } catch (e) {
      testResult = errMsg(e);
    } finally {
      refreshing = false;
    }
  }

  async function testConnection() {
    if (!s) return;
    testing = true;
    testResult = null;
    try {
      s = await api.setSettings(s);
      const result = await api.testAi();
      testResult = result;
    } catch (e) {
      testResult = errMsg(e);
    } finally {
      testing = false;
    }
  }

  onMount(async () => {
    try {
      s = await api.getSettings();
      currentProvider = detectProvider(s?.ai_base_url ?? "");
      modelList = [...(PROVIDERS[currentProvider]?.models ?? [])];
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
      <span>Capture clipboard history</span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.spellcheck_enabled} />
      <span>Spellcheck <span class="faint">— auto-correct typos</span></span>
    </label>
    <label class="toggle">
      <input type="checkbox" bind:checked={s.case_adapt_enabled} />
      <span>Case adaptation <span class="faint">— SIG → ALL CAPS</span></span>
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
      <span>Enable AI macros <span class="faint">— transform text via LLM</span></span>
    </label>

    {#if s.ai_enabled}
      <div class="row" style="gap:10px">
        <div class="field" style="flex:1">
          <label for="ai-provider">Provider</label>
          <select id="ai-provider" value={currentProvider} onchange={onProviderChange}>
            {#each Object.entries(PROVIDERS) as [key, p] (key)}
              <option value={key}>{p.name}</option>
            {/each}
          </select>
        </div>
        <div class="field" style="flex:1">
          <label for="ai-model">Model</label>
          <div class="row" style="gap:4px">
            {#if modelList.length > 0}
              <select id="ai-model" bind:value={s.ai_model} style="flex:1">
                {#each modelList as m (m)}
                  <option value={m}>{m}</option>
                {/each}
              </select>
            {:else}
              <input bind:value={s.ai_model} placeholder="model name" style="flex:1" />
            {/if}
            <button class="tiny" onclick={refreshModels} disabled={refreshing} title="Fetch available models from the provider">
              {refreshing ? "…" : "🔄"}
            </button>
          </div>
        </div>
      </div>

      {#if currentProvider === "custom"}
        <div class="field">
          <label for="ai-url">API base URL</label>
          <input id="ai-url" bind:value={s.ai_base_url} placeholder="https://api.example.com/v1" />
        </div>
      {/if}

      <div class="field">
        <label for="ai-key">API key</label>
        <div class="row" style="gap:6px">
          <input id="ai-key" type="password" bind:value={s.ai_api_key} placeholder="Enter your API key…" style="flex:1" />
          <button class="tiny" onclick={testConnection} disabled={testing} title="Test the connection">
            {testing ? "…" : "✓ Test"}
          </button>
        </div>
      </div>

      {#if testResult}
        <div class={testResult.startsWith("✅") || testResult.startsWith("Loaded") ? "ok-msg" : "error"}>{testResult}</div>
      {/if}

      <span class="faint">
        Get a Groq API key at <code>console.groq.com/keys</code> (free tier available).
        DeepSeek at <code>platform.deepseek.com</code>. OpenAI at <code>platform.openai.com</code>.
      </span>
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
  .toggle { display: flex; gap: 10px; align-items: flex-start; cursor: pointer; }
  .toggle input { width: auto; margin-top: 3px; }
  .divider { height: 1px; background: var(--border-soft); margin: 6px 0; }
  .ok-msg { color: var(--ok); font-size: 13px; }
</style>
