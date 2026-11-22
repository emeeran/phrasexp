<script lang="ts">
  // Status bar — counts + listener state. Dense type, hairline top border.
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { clipboardStore } from "$lib/stores/clipboard.svelte";
  import { statusStore } from "$lib/stores/status.svelte";
</script>

<div class="statusbar">
  <span>{phrasesStore.items.length} phrases</span>
  <span>·</span>
  <span>{foldersStore.items.length} folders</span>
  <span>·</span>
  <span>{clipboardStore.items.length} clips</span>
  {#if statusStore.autotext?.enabled}
    <span>·</span>
    <span class="ok">autotext on</span>
  {/if}
  <span class="spacer"></span>
  <span class="faint">PhraseXp</span>
</div>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: 4px 14px;
    background: var(--surface-1);
    border-top: 1px solid var(--border-soft);
    font-size: var(--text-dense);
    color: var(--text-dim);
    user-select: none;
  }
  .ok {
    color: var(--ok);
  }
  .spacer {
    flex: 1;
  }
</style>
