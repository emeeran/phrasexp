<script lang="ts">
  // Spellcheck panel — ported from the legacy Spellcheck tab.
  import TagInput from "$lib/components/TagInput.svelte";
  import type { Settings } from "$lib/types";

  let { s }: { s: Settings } = $props();
</script>

<div class="col">
  <label class="toggle">
    <span class="toggle-text">Spellcheck <span class="faint">— auto-correct typos as you type</span></span>
    <span class="switch"><input type="checkbox" bind:checked={s.spellcheck_enabled} /><span class="slider"></span></span>
  </label>
  {#if s.spellcheck_enabled}
    <div class="grid">
      <div class="field">
        <label for="sc-mode">Aggressiveness</label>
        <select id="sc-mode" bind:value={s.spellcheck_mode}>
          <option value="conservative">Conservative — distance 1 only</option>
          <option value="normal">Normal — distance 2 + Soundex for long words</option>
          <option value="aggressive">Aggressive — distance 3 + Soundex for all</option>
        </select>
      </div>
      <div class="field">
        <label for="sc-delivery">Delivery</label>
        <select id="sc-delivery" bind:value={s.spellcheck_delivery}>
          <option value="silent">Silent — auto-correct as you type</option>
          <option value="offer">Offer — pop up candidates; correct only on click</option>
        </select>
      </div>
    </div>
    <div class="grid">
      <div class="field">
        <label for="sc-ignore">Ignore words <span class="faint">(Enter to add)</span></label>
        <TagInput id="sc-ignore" bind:value={s.spellcheck_ignore} placeholder="add a word…" />
      </div>
      <div class="field">
        <label for="sc-exclude">Exclude apps <span class="faint">(window-title substrings; Enter to add)</span></label>
        <TagInput id="sc-exclude" bind:value={s.spellcheck_exclude_apps} placeholder="e.g. Visual Studio Code" />
      </div>
    </div>
    <div class="note">
      <strong>Spellcheck engine:</strong> Damerau-Levenshtein + SymSpell delete-index over a
      frequency-ranked 65K dictionary, Soundex phonetic fallback, QWERTY-weighted tiebreak,
      bigram context ranking, learned personal corrections, case preservation, abbreviation
      protection.
    </div>
  {/if}
</div>

<style>
  .col {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    align-items: start;
    column-gap: var(--sp-5);
    row-gap: var(--sp-1);
  }
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    cursor: pointer;
    padding: 1px 0;
  }
  .toggle-text { flex: 1; }
  .toggle-text .faint { display: block; font-size: var(--fs-xs); font-weight: 400; }
  .note {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    line-height: 1.5;
    padding: 8px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
</style>
