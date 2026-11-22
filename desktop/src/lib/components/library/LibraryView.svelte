<script lang="ts">
  // LibraryView — the redesigned Library: FolderTree | PhraseList | DetailPane.
  // All three columns are drag-resizable; widths persist in localStorage.
  import { api, errMsg } from "$lib/invoke";
  import { hdrag } from "$lib/hdrag";
  import { app } from "$lib/stores/app.svelte";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import type { PhraseSummary } from "$lib/types";
  import QuickAdd from "$lib/components/QuickAdd.svelte";
  import Icon from "../common/Icon.svelte";
  import FolderTree from "./FolderTree.svelte";
  import PhraseList from "./PhraseList.svelte";
  import DetailPane from "./DetailPane.svelte";

  // Column widths: folders (150–360) and phrase list (260–min 40% of remaining).
  // Middle flexes, so only the two fixed sides persist; clamps keep the layout sane.
  let colFolders = $state(Number(localStorage.getItem("phrasexp-col-folders")) || 200);
  let colList = $state(Number(localStorage.getItem("phrasexp-col-list")) || 340);
  $effect(() => {
    localStorage.setItem("phrasexp-col-folders", String(colFolders));
    localStorage.setItem("phrasexp-col-list", String(colList));
  });

  // Folder drawer: collapsible to a thin rail so list + detail get full width.
  // Default open ("0" is the stored collapsed value).
  let drawerOpen = $state(localStorage.getItem("phrasexp-drawer-open") !== "0");
  $effect(() => {
    localStorage.setItem("phrasexp-drawer-open", drawerOpen ? "1" : "0");
  });

  const maxList = () => Math.max(260, window.innerWidth - colFolders - 200);
  const dragFolders = hdrag({
    get: () => colFolders,
    onDrag: (dx, start) => (colFolders = Math.max(150, Math.min(360, start + dx))),
  });
  const dragList = hdrag({
    get: () => colList,
    onDrag: (dx, start) => (colList = Math.max(260, Math.min(maxList(), start - dx))),
  });

  async function editPhrase(p: PhraseSummary) {
    try {
      const full = await api.getPhrase(p.id);
      app.editing = { phrase: full, folderId: full.folder_id };
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }
</script>

<div class="library">
  <div class="quickadd-bar">
    <QuickAdd
      folders={foldersStore.items}
      selectedFolderId={phrasesStore.selectedFolderId}
      onadded={() => { void foldersStore.load(); void phrasesStore.load(); }}
    />
  </div>
  <div class="cols">
    {#if drawerOpen}
      <div class="folders" style={`width:${colFolders}px`}>
        <FolderTree oncollapse={() => (drawerOpen = false)} />
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="handle"
        role="separator"
        aria-orientation="vertical"
        tabindex="-1"
        aria-label="Resize folder column"
        use:dragFolders
      ></div>
    {:else}
      <div class="drawer-rail">
        <button
          class="rail-btn"
          aria-expanded={drawerOpen}
          aria-label="Show folders panel"
          title="Show folders panel"
          onclick={() => (drawerOpen = true)}
        >
          <Icon name="library" size={15} />
        </button>
      </div>
    {/if}
    <div class="list-col" style={`width:${colList}px`}>
      <PhraseList onedit={editPhrase} />
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="handle"
      role="separator"
      aria-orientation="vertical"
      tabindex="-1"
      aria-label="Resize phrase column"
      use:dragList
    ></div>
    <DetailPane />
  </div>
</div>

<style>
  .library {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .quickadd-bar {
    padding: 10px 14px 6px;
    border-bottom: 1px solid var(--border-soft);
    background: var(--surface-0);
  }
  .cols {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .folders {
    flex: 0 0 auto;
    min-width: 0;
    border-right: 1px solid var(--border-soft);
    background: var(--surface-1);
    overflow: hidden;
  }
  .drawer-rail {
    flex: 0 0 34px;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 10px;
    border-right: 1px solid var(--border-soft);
    background: var(--surface-1);
  }
  .rail-btn {
    width: 26px;
    height: 26px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-dim);
    line-height: 0;
    transition: background var(--t-fast), color var(--t-fast), border-color var(--t-fast);
  }
  .rail-btn:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .rail-btn:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .handle {
    flex: 0 0 auto;
    width: 4px;
    cursor: col-resize;
    background: transparent;
    transition: background var(--t-fast);
  }
  .handle:hover,
  .handle:active {
    background: var(--accent);
  }
  .list-col {
    flex: 0 0 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface-0);
  }
</style>
