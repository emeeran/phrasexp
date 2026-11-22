<script lang="ts">
  // Input-form field renderer shared by TestPanel and PopupInputs — same markup,
  // different id prefix and bound record. Mutating `inputs` updates the parent's
  // $state record in place.
  import type { InputField } from "$lib/macros";

  let {
    fields,
    inputs,
    idPrefix,
  }: {
    fields: InputField[];
    inputs: Record<string, string>;
    idPrefix: string;
  } = $props();
</script>

{#each fields as f (f.field)}
  <div class="field">
    <label for={`${idPrefix}${f.field}`}>{f.label ?? f.field}</label>
    {#if f.kind === "checkbox"}
      <input id={`${idPrefix}${f.field}`} type="checkbox" style="width:auto"
        checked={inputs[f.field] === "true"}
        onchange={(e) => (inputs[f.field] = e.currentTarget.checked ? "true" : "false")} />
    {:else if f.kind === "select"}
      <select id={`${idPrefix}${f.field}`} bind:value={inputs[f.field]}>
        {#each f.options as opt (opt)}
          <option value={opt}>{opt}</option>
        {/each}
      </select>
    {:else}
      <input id={`${idPrefix}${f.field}`} bind:value={inputs[f.field]} />
    {/if}
  </div>
{/each}
