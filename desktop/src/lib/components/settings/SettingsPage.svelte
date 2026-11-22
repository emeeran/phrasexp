<script lang="ts">
  // Settings page — section nav + panels, fed by the settings store. The page owns
  // a draft copy; Save writes through (Rust applies side-effects). AI has its own
  // page now; Macros reference lives on the Macros page.
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { useDraft } from "$lib/stores/draft.svelte";
  import type { Settings } from "$lib/types";
  import FeaturesPanel from "./FeaturesPanel.svelte";
  import SpellcheckPanel from "./SpellcheckPanel.svelte";
  import DataPanel from "./DataPanel.svelte";
  import FoldersPanel from "./FoldersPanel.svelte";
  import ShortcutsPanel from "./ShortcutsPanel.svelte";
  import AboutPanel from "./AboutPanel.svelte";
  import Skeleton from "../common/Skeleton.svelte";

  const SECTIONS = [
    { id: "features", label: "Features" },
    { id: "spellcheck", label: "Spellcheck" },
    { id: "data", label: "Data" },
    { id: "folders", label: "Folders" },
    { id: "shortcuts", label: "Shortcuts" },
    { id: "about", label: "About" },
  ] as const;
  type SectionId = (typeof SECTIONS)[number]["id"];

  let active = $state<SectionId>("features");
  let saving = $state(false);
  // Destructuring the Draft object would invoke the getter once and snapshot null;
  // $derived re-reads `.draft` reactively (same pattern as AiPage).
  const settingsDraft = useDraft<Settings>(() => settingsStore.values);
  const draft = $derived(settingsDraft.draft);

  async function save() {
    if (!draft) return;
    saving = true;
    await settingsStore.save(draft);
    saving = false;
  }
</script>

<div class="settings">
  <div class="head">
    <h2>Settings</h2>
    <button class="primary" onclick={save} disabled={saving || !draft}>
      {saving ? "Saving…" : "Save"}
    </button>
  </div>

  {#if !draft}
    <div class="pad"><Skeleton rows={6} /></div>
  {:else}
    <div class="body">
      <nav class="nav" aria-label="Settings sections">
        {#each SECTIONS as sec (sec.id)}
          <button
            class="nav-item"
            class:on={active === sec.id}
            aria-current={active === sec.id ? "true" : undefined}
            onclick={() => (active = sec.id)}
          >
            {sec.label}
          </button>
        {/each}
      </nav>
      <div class="panel scroll">
        {#if active === "features"}
          <FeaturesPanel s={draft} />
        {:else if active === "spellcheck"}
          <SpellcheckPanel s={draft} />
        {:else if active === "data"}
          <DataPanel />
        {:else if active === "folders"}
          <FoldersPanel />
        {:else if active === "shortcuts"}
          <ShortcutsPanel />
        {:else if active === "about"}
          <AboutPanel />
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px 6px;
  }
  .head h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-display);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px 6px;
    border-right: 1px solid var(--border-soft);
    background: var(--surface-1);
  }
  .nav-item {
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    font-size: var(--fs-base);
    color: var(--text-dim);
    transition: background var(--t-fast), color var(--t-fast);
  }
  .nav-item:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .nav-item.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 500;
  }
  .panel {
    flex: 1;
    min-width: 0;
    padding: 10px 14px;
    max-width: 980px;
    /* Wide screens flow the panel's cards two-up via the panel content's own
       grid (see each panel's .col); this wrapper just gives it room. */
  }
  .pad {
    padding: 14px;
  }
</style>
