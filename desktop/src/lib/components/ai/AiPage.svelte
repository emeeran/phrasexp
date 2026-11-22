<script lang="ts">
  // AI page — provider config + model list + connection test, ported from the
  // legacy Settings → AI tab with provider presets.
  import { api, errMsg } from "$lib/invoke";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { useDraft } from "$lib/stores/draft.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import Skeleton from "../common/Skeleton.svelte";
  import HotkeyInput from "../common/HotkeyInput.svelte";
  import AiActionsEditor from "./AiActionsEditor.svelte";
  import type { Settings } from "$lib/types";

  const PROVIDERS: Record<string, { name: string; baseUrl: string; models: string[] }> = {
    groq: { name: "Groq", baseUrl: "https://api.groq.com/openai/v1", models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "gemma2-9b-it"] },
    deepseek: { name: "DeepSeek", baseUrl: "https://api.deepseek.com/v1", models: ["deepseek-chat", "deepseek-reasoner"] },
    openai: { name: "OpenAI", baseUrl: "https://api.openai.com/v1", models: ["gpt-4o-mini", "gpt-4o", "gpt-3.5-turbo"] },
    custom: { name: "Custom", baseUrl: "", models: [] },
  };

  let currentProvider = $state("groq");
  let modelList = $state<string[]>([]);
  let testing = $state(false);
  let refreshing = $state(false);
  let testResult = $state<string | null>(null);

  const settingsDraft = useDraft<Settings>(() => settingsStore.values);
  const draft = $derived(settingsDraft.draft);
  // First copy seeds the provider select + model list for the detected provider.
  $effect(() => {
    const d = draft;
    if (d && modelList.length === 0) {
      currentProvider = detectProvider(d.ai_base_url ?? "");
      modelList = [...(PROVIDERS[currentProvider]?.models ?? [])];
    }
  });

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
    if (draft && key !== "custom") {
      draft.ai_base_url = PROVIDERS[key].baseUrl;
      modelList = [...PROVIDERS[key].models];
      if (!modelList.includes(draft.ai_model ?? "")) draft.ai_model = modelList[0] ?? "";
    }
  }

  async function save() {
    if (!draft) return;
    await settingsStore.save(draft);
  }

  async function refreshModels() {
    if (!draft) return;
    refreshing = true;
    testResult = null;
    try {
      await save();
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
    if (!draft) return;
    testing = true;
    testResult = null;
    try {
      await save();
      const result = await api.testAi();
      testResult = result;
    } catch (e) {
      testResult = errMsg(e);
    } finally {
      testing = false;
    }
  }
</script>

<div class="page">
  <div class="head">
    <h2>AI</h2>
    <p class="muted">
      AI macros send your prompt to an LLM and splice the answer into the expansion.
      The API key is encrypted at rest when encryption is enabled.
    </p>
  </div>

  {#if !draft}
    <div class="pad"><Skeleton rows={5} /></div>
  {:else}
    <div class="body scroll">
      <label class="toggle">
        <span class="toggle-text">Enable AI macros <span class="faint">— transform text via LLM</span></span>
        <span class="switch"><input type="checkbox" bind:checked={draft.ai_enabled} /><span class="slider"></span></span>
      </label>

      {#if draft.ai_enabled}
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
                <select id="ai-model" bind:value={draft.ai_model} style="flex:1">
                  {#each modelList as m (m)}
                    <option value={m}>{m}</option>
                  {/each}
                </select>
              {:else}
                <input bind:value={draft.ai_model} placeholder="model name" style="flex:1" />
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
            <input id="ai-url" bind:value={draft.ai_base_url} placeholder="https://api.example.com/v1" />
          </div>
        {/if}
        <div class="field">
          <label for="ai-key">API key</label>
          <div class="row" style="gap:6px">
            <input id="ai-key" type="password" bind:value={draft.ai_api_key} placeholder="Enter your API key…" style="flex:1" />
            <button class="tiny" onclick={testConnection} disabled={testing} title="Test the connection">
              {testing ? "…" : "✓ Test"}
            </button>
          </div>
        </div>
        <div class="field">
          <label for="ai-voice">Brand voice (optional)</label>
          <textarea
            id="ai-voice"
            rows={2}
            bind:value={draft.ai_instructions}
            placeholder="e.g. Keep it short, professional, and jargon-free — British English"
          ></textarea>
          <span class="faint" style="font-size:var(--fs-xs)">
            Sent as a system message with every AI call, so all actions share one style.
          </span>
        </div>
        <div class="field">
          <label for="ai-sel-hotkey">Selection rewrite hotkey</label>
          <div id="ai-sel-hotkey">
            <HotkeyInput
              value={draft.ai_selection_hotkey ?? ""}
              onchange={(v) => {
                if (draft) draft.ai_selection_hotkey = v;
              }}
            />
          </div>
          <span class="faint" style="font-size:var(--fs-xs)">
            Select text anywhere, press it, pick an action — then Replace (over the
            selection), Insert (after it), Retry, or Discard. Click Record and press
            the chord (Esc cancels), or clear with ✕.
            Takes effect after Save. Beware: app-specific global hotkeys (e.g. Obsidian
            Clipper) can silently take the same chord.
          </span>
        </div>
        <div class="row" style="justify-content:flex-end">
          <button class="primary" onclick={save}>Save</button>
        </div>
        {#if testResult}
          <div class={testResult.startsWith("AI ok") || testResult.startsWith("Loaded") ? "ok-msg" : "error"}>{testResult}</div>
        {/if}
        <span class="faint">
          Groq key: <code>console.groq.com/keys</code> · DeepSeek: <code>platform.deepseek.com</code> · OpenAI: <code>platform.openai.com</code>
        </span>

        <div class="actions-section">
          <AiActionsEditor />
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .head {
    padding: 14px 18px 10px;
  }
  .head h2 {
    margin: 0 0 4px;
    font-family: var(--font-display);
    font-size: var(--text-display);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
  }
  .head p {
    margin: 0;
    max-width: 70ch;
    font-size: var(--fs-base);
  }
  .pad {
    padding: 20px;
  }
  .body {
    flex: 1;
    padding: 4px 18px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    max-width: 720px;
  }
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    cursor: pointer;
    padding: 3px 0;
  }
  .toggle-text { flex: 1; }
  .toggle-text .faint { display: block; font-size: var(--fs-xs); font-weight: 400; }
  .ok-msg { color: var(--ok); font-size: var(--fs-base); }
  .actions-section {
    border-top: 1px solid var(--border-soft);
    padding-top: 12px;
    margin-top: 4px;
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    background: var(--surface-2);
    padding: 1px 5px;
    border-radius: var(--radius-xs);
  }
</style>
