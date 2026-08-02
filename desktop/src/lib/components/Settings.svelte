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
  let activeTab = $state("features");

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

  const TABS = [
    { id: "features", label: "Features" },
    { id: "ai", label: "AI" },
    { id: "spellcheck", label: "Spellcheck" },
    { id: "data", label: "Data" },
    { id: "shortcuts", label: "Shortcuts" },
    { id: "about", label: "About" },
  ];

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
      s = await api.setSettings(s);
      const models = await api.listAiModels();
      modelList = models.sort();
      testResult = `Loaded ${models.length} models`;
    } catch (e) { testResult = errMsg(e); }
    finally { refreshing = false; }
  }

  async function testConnection() {
    if (!s) return;
    testing = true;
    testResult = null;
    try {
      s = await api.setSettings(s);
      const result = await api.testAi();
      testResult = result;
    } catch (e) { testResult = errMsg(e); }
    finally { testing = false; }
  }

  onMount(async () => {
    try {
      s = await api.getSettings();
      currentProvider = detectProvider(s?.ai_base_url ?? "");
      modelList = [...(PROVIDERS[currentProvider]?.models ?? [])];
    } catch (e) { error = errMsg(e); }
  });

  async function save() {
    if (!s) return;
    saving = true;
    error = null;
    try {
      s = await api.setSettings(s);
      savedFlash = true;
      setTimeout(() => (savedFlash = false), 1500);
    } catch (e) { error = errMsg(e); }
    finally { saving = false; }
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

  const SHORTCUTS = [
    { key: "Ctrl+Alt+Space", desc: "Open the search popup (searchable phrase picker)" },
    { key: "Control+Shift+S", desc: "Toggle spellcheck on/off" },
    { key: "↑ ↓ Enter", desc: "Navigate popup lists (inside the popup window)" },
    { key: "Esc", desc: "Dismiss the popup" },
    { key: "Tab", desc: "Accept popup selection / input-form confirm" },
    { key: "Any abbreviation", desc: "Type it in any app → auto-expands (when autotext is on)" },
    { key: "Per-phrase hotkey", desc: "Set in the phrase editor → directly injects that phrase" },
    { key: "Type word + Space", desc: "Spellcheck auto-corrects typos (when enabled)" },
  ];
</script>

<div class="settings">
  <div class="row between" style="margin-bottom:4px">
    <h3 style="margin:0">Settings</h3>
    <button class="ghost tiny" onclick={() => { if (s) save(); }} disabled={saving || !s}>
      {saving ? "Saving…" : "💾 Save"}
    </button>
  </div>

  {#if s}
    <div class="tabs">
      {#each TABS as tab (tab.id)}
        <button class="tab" class:on={activeTab === tab.id} onclick={() => (activeTab = tab.id)}>
          {tab.label}
        </button>
      {/each}
    </div>

    <div class="tab-body">
      <!-- Features -->
      {#if activeTab === "features"}
        <label class="toggle">
          <input type="checkbox" bind:checked={s.autotext_enabled} />
          <span>Silent autotext <span class="faint">— expand abbreviations as you type</span></span>
        </label>
        <label class="toggle">
          <input type="checkbox" bind:checked={s.clipboard_enabled} />
          <span>Capture clipboard history</span>
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

      <!-- AI -->
      {:else if activeTab === "ai"}
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
                <button class="tiny" onclick={refreshModels} disabled={refreshing} title="Fetch available models">
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
            Groq key: <code>console.groq.com/keys</code> · DeepSeek: <code>platform.deepseek.com</code> · OpenAI: <code>platform.openai.com</code>
          </span>
        {/if}

      <!-- Spellcheck -->
      {:else if activeTab === "spellcheck"}
        <label class="toggle">
          <input type="checkbox" bind:checked={s.spellcheck_enabled} />
          <span>Spellcheck <span class="faint">— auto-correct typos as you type</span></span>
        </label>
        {#if s.spellcheck_enabled}
          <div class="field">
            <label for="sc-mode">Aggressiveness</label>
            <select id="sc-mode" bind:value={s.spellcheck_mode}>
              <option value="conservative">Conservative — distance 1 only</option>
              <option value="normal">Normal — distance 2 + Soundex for long words</option>
              <option value="aggressive">Aggressive — distance 3 + Soundex for all</option>
            </select>
          </div>
          <div class="field">
            <label for="sc-ignore">Ignore words <span class="faint">(comma-separated)</span></label>
            <input id="sc-ignore" bind:value={s.spellcheck_ignore} placeholder="docker, nginx, kubernetes" />
          </div>
          <div class="note">
            <strong>Spellcheck features:</strong> Damerau-Levenshtein (transposition detection), 104K-word system dictionary, BK-tree O(log n) lookup, Soundex phonetic fallback, frequency-weighted ranking, case preservation, abbreviation protection, learn-from-rejection.
          </div>
        {/if}

      <!-- Data -->
      {:else if activeTab === "data"}
        <div class="col" style="gap:8px">
          <button onclick={exportLib}>⭳ Export library as JSON</button>
          <button onclick={() => csvInput?.click()}>⭱ Import phrases from CSV</button>
          <button onclick={() => ahkInput?.click()}>⭱ Import AutoHotKey hotstrings</button>
          <button class="danger" onclick={doReset}>🗑 Reset database (delete all data)</button>
        </div>
        <input bind:this={csvInput} type="file" accept=".csv,text/csv" style="display:none" onchange={onCsvPicked} />
        <input bind:this={ahkInput} type="file" accept=".ahk,.txt" style="display:none" onchange={onAhkPicked} />

      <!-- Shortcuts -->
      {:else if activeTab === "shortcuts"}
        <table class="shortcut-table">
          <thead>
            <tr><th>Shortcut</th><th>Action</th></tr>
          </thead>
          <tbody>
            {#each SHORTCUTS as sc}
              <tr>
                <td><code>{sc.key}</code></td>
                <td>{sc.desc}</td>
              </tr>
            {/each}
          </tbody>
        </table>

      <!-- About -->
      {:else if activeTab === "about"}
        <div class="about">
          <h2 style="margin:0 0 4px">PhraseXp</h2>
          <span class="faint">v0.1.1 · Tauri 2 + Rust + Svelte 5</span>
          <p>Lightweight, local-first text expander. Type an abbreviation to expand it, summon a search popup with a global hotkey, or use macros, Markdown, AI, and spellcheck to automate your typing.</p>
          <div class="divider"></div>
          <p class="about-credit">Developed by <strong>Dawnstar</strong></p>
          <div class="row" style="gap:12px; margin-top:8px">
            <a href="https://github.com/emeeran/phrasexp" target="_blank" rel="noopener">GitHub ↗</a>
          </div>
        </div>
      {/if}

      {#if error}<div class="error" style="margin-top:8px">{error}</div>{/if}
      {#if savedFlash}<div class="ok-msg" style="margin-top:8px">✓ Settings saved</div>{/if}
    </div>
  {:else}
    <div class="faint">Loading…</div>
  {/if}
</div>

<style>
  .settings { display: flex; flex-direction: column; gap: 6px; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--border-soft); margin-bottom: 8px; flex-wrap: wrap; }
  .tab {
    padding: 5px 12px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    font-size: 13px;
    color: var(--text-dim);
  }
  .tab:hover { color: var(--text); }
  .tab.on { color: var(--accent); border-bottom-color: var(--accent); }
  .tab-body { display: flex; flex-direction: column; gap: 10px; min-height: 120px; }
  .toggle { display: flex; gap: 10px; align-items: flex-start; cursor: pointer; }
  .toggle input { width: auto; margin-top: 3px; }
  .divider { height: 1px; background: var(--border-soft); margin: 6px 0; }
  .ok-msg { color: var(--ok); font-size: 13px; }
  .note { font-size: 12px; color: var(--text-dim); line-height: 1.5; padding: 8px; border: 1px solid var(--border-soft); border-radius: var(--radius-sm); }
  .shortcut-table { width: 100%; border-collapse: collapse; font-size: 13px; }
  .shortcut-table th { text-align: left; color: var(--text-dim); padding: 4px 8px; border-bottom: 1px solid var(--border-soft); font-size: 12px; }
  .shortcut-table td { padding: 5px 8px; border-bottom: 1px solid var(--border-soft); }
  .shortcut-table code { background: var(--bg-elev-2); padding: 2px 6px; border-radius: 4px; font-family: ui-monospace, monospace; font-size: 12px; }
  .about { display: flex; flex-direction: column; gap: 4px; }
  .about p { margin: 4px 0; font-size: 13px; color: var(--text-dim); line-height: 1.5; }
  .about-credit { font-size: 14px !important; color: var(--text) !important; }
  .about a { color: var(--accent); font-size: 13px; }
</style>
