<script lang="ts">
  let {
    variations = $bindable([]),
    variationPicks = $bindable(new Set<number>()),
    onapply,
  }: {
    variations?: string[];
    variationPicks?: Set<number>;
    /** Parent keeps the undo push: swaps the first text segment for a pick macro. */
    onapply: () => void;
  } = $props();

  function toggleVariation(i: number) {
    const next = new Set(variationPicks);
    if (!next.delete(i)) next.add(i);
    variationPicks = next;
  }
</script>

{#if variations.length > 0}
  <div class="segment">
    <div class="row between">
      <strong>Pick the variations to keep</strong>
      <button class="tiny ghost" onclick={() => (variations = [])}>Discard</button>
    </div>
    <span class="faint" style="font-size:var(--fs-xs)">
      The first text segment becomes a pick-random macro: original + kept variations.
    </span>
    {#each variations as v, i (i)}
      <label class="row" style="gap:8px; cursor:pointer; align-items:flex-start">
        <input
          type="checkbox"
          checked={variationPicks.has(i)}
          onchange={() => toggleVariation(i)}
          style="width:auto; margin-top:3px"
        />
        <span>{v}</span>
      </label>
    {/each}
    <div class="row" style="justify-content:flex-end">
      <button class="tiny primary" disabled={variationPicks.size === 0} onclick={onapply}>
        Apply {variationPicks.size || ""}
      </button>
    </div>
  </div>
{/if}

<style>
  .segment {
    background: var(--bg-elev);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    padding: var(--sp-3);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    transition: border-color var(--t-fast), box-shadow var(--t-fast);
  }
  .segment:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
  }
</style>
