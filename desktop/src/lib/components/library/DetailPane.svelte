<script lang="ts">
  // DetailPane — resizable right pane: phrase editor when editing, else the Test
  // panel. Width persists under the legacy `phrasexp-col3` key so saved widths survive.
  import { app } from "$lib/stores/app.svelte";
  import { hdrag } from "$lib/hdrag";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import PhraseEditor from "$lib/components/editor/PhraseEditor.svelte";
  import TestPanel from "./TestPanel.svelte";

  let width = $state(Number(localStorage.getItem("phrasexp-col3")) || 380);
  $effect(() => localStorage.setItem("phrasexp-col3", String(width)));

  const dragHandle = hdrag({
    get: () => width,
    onDrag: (dx, start) => (width = Math.max(260, Math.min(640, start - dx))),
  });
</script>

<div class="detail" style={`width:${width}px`}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="handle" role="separator" aria-orientation="vertical" tabindex="-1" use:dragHandle></div>
  <div class="content scroll">
    {#if app.editing}
      {#key app.editing}
        <PhraseEditor
          phrase={app.editing.phrase}
          folderId={app.editing.folderId}
          initialText={app.editing.initialText}
          folders={foldersStore.items}
          phrases={phrasesStore.items}
          onsaved={() => { app.editing = null; void phrasesStore.load(); }}
          oncancel={() => (app.editing = null)}
        />
      {/key}
    {:else}
      <TestPanel />
    {/if}
  </div>
</div>

<style>
  .detail {
    display: flex;
    flex: 0 0 auto;
    min-width: 0;
    border-left: 1px solid var(--border-soft);
    background: var(--surface-1);
  }
  .handle {
    width: 4px;
    flex: 0 0 auto;
    cursor: col-resize;
    background: transparent;
    transition: background var(--t-fast);
  }
  .handle:hover,
  .handle:active {
    background: var(--accent);
  }
  .content {
    flex: 1;
    min-width: 0;
    padding: 14px;
  }
</style>
