<script lang="ts">
  // Inputs mode — fill inputform fields for the chosen phrase.
  import { fieldsOf } from "$lib/macros";
  import type { Phrase } from "$lib/types";
  import InputFields from "$lib/components/common/InputFields.svelte";

  let {
    current,
    formInputs = $bindable({}),
    busy,
    onconfirm,
    onback,
  }: {
    current: Phrase;
    formInputs?: Record<string, string>;
    busy: boolean;
    onconfirm: () => void;
    onback: () => void;
  } = $props();
</script>

<div class="inputs">
  <div class="row between head" data-tauri-drag-region>
    <strong data-tauri-drag-region>{current.name}</strong>
    <button class="ghost tiny" onclick={onback}>‹ back</button>
  </div>
  <div class="fields scroll">
    <InputFields fields={fieldsOf(current)} inputs={formInputs} idPrefix="fld-" />
  </div>
  <div class="row" style="justify-content:flex-end; margin-top:6px">
    <button class="primary" onclick={onconfirm} disabled={busy}>
      {busy ? "…" : "Insert"}
    </button>
  </div>
</div>

<style>
  .inputs {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .head {
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 300px;
    min-height: 0;
  }
</style>
