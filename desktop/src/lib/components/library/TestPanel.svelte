<script lang="ts">
  // TestPanel — resolve a trigger and preview the expansion (inputform fields are
  // collected first, exactly like the popup flow).
  import { api, errMsg } from "$lib/invoke";
  import { fieldsOf, defaultsFor, type InputField } from "$lib/macros";
  import InputFields from "$lib/components/common/InputFields.svelte";

  let expanderType = $state<"autotext" | "hotkey">("autotext");
  let expanderValue = $state("br,");
  let expandOutput = $state("");
  let expandError = $state<string | null>(null);
  let testFields = $state<InputField[]>([]);
  let testInputs = $state<Record<string, string>>({});

  async function doExpand() {
    expandError = null;
    expandOutput = "";
    const value = expanderValue.trim();
    if (!value) return;
    try {
      const phrase = await api.resolvePhrase(expanderType, value);
      if (!phrase) {
        expandError = "No phrase matches that trigger.";
        return;
      }
      const fields = fieldsOf(phrase);
      if (fields.length > 0) {
        testFields = fields;
        testInputs = defaultsFor(fields);
        return;
      }
      testFields = [];
      await runExpand(value, {});
    } catch (e) {
      expandError = errMsg(e);
    }
  }

  async function runExpand(value: string, inputs: Record<string, string>) {
    try {
      const res = await api.expand(expanderType, value, inputs);
      expandOutput = res.output;
    } catch (e) {
      expandError = errMsg(e);
    }
  }

  async function expandWithInputs() {
    expandError = null;
    expandOutput = "";
    await runExpand(expanderValue.trim(), testInputs);
  }

  function cancelInputs() {
    testFields = [];
    testInputs = {};
  }
</script>

<div class="col" style="gap:12px">
  <h3 style="margin:0">Test expansion</h3>
  <p class="muted" style="margin:0">
    Resolves a trigger against the library and returns the expanded text.
  </p>
  <div class="row">
    <select bind:value={expanderType} style="flex:0 0 120px" aria-label="Trigger type">
      <option value="autotext">autotext</option>
      <option value="hotkey">hotkey</option>
    </select>
    <input bind:value={expanderValue} placeholder={expanderType === "autotext" ? "br," : "ctrl+shift+b"} aria-label="Trigger value" />
    <button class="primary" onclick={doExpand}>Expand</button>
  </div>
  {#if testFields.length > 0}
    <div class="inputs">
      <div class="row between">
        <span class="muted">This phrase has form fields — fill them in:</span>
        <button class="ghost tiny" onclick={cancelInputs} aria-label="Cancel input form">✕</button>
      </div>
      <InputFields fields={testFields} inputs={testInputs} idPrefix="t-" />
      <div class="row" style="justify-content:flex-end; gap:8px">
        <button class="ghost tiny" onclick={cancelInputs}>Cancel</button>
        <button class="primary" onclick={expandWithInputs}>Expand with inputs</button>
      </div>
    </div>
  {/if}
  {#if expandOutput}
    <div class="output">{expandOutput}</div>
  {/if}
  {#if expandError}<div class="error">{expandError}</div>{/if}
</div>

<style>
  .inputs {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    background: var(--surface-0);
  }
  .output {
    background: var(--surface-0);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 10px;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
