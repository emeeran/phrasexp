<script lang="ts">
  import { api, errMsg } from "$lib/invoke";
  import type { Folder } from "$lib/types";

  let {
    folders,
    selectedFolderId = null,
    onadded,
  }: {
    folders: Folder[];
    selectedFolderId?: string | null;
    onadded: () => void;
  } = $props();

  let abbr = $state("");
  let expansion = $state("");
  let folderId = $state("");
  let error = $state<string | null>(null);
  let saving = $state(false);
  let expEl: HTMLInputElement | undefined = $state();

  // Default to the selected folder (or the first one) once.
  $effect(() => {
    if (!folderId) folderId = selectedFolderId ?? folders[0]?.id ?? "";
  });

  async function add() {
    error = null;
    const a = abbr.trim();
    const text = expansion;
    if (!a) return (error = "Enter a trigger, e.g. /emg");
    if (!text) return (error = "Enter the text to expand to");

    saving = true;
    try {
      // No folder yet → create a default one so quick-add never blocks on folder setup.
      if (!folderId) {
        const f = await api.createFolder({ name: "Snippets" });
        folderId = f.id;
      }
      await api.createPhrase({
        folderId,
        name: text.length > 80 ? text.slice(0, 80) + "…" : text,
        abbreviation: a,
        body: [{ type: "text", text }],
        triggers: [{ type: "autotext", abbreviation: a }],
      });
      abbr = "";
      expansion = "";
      abbrEl?.focus();
      onadded();
    } catch (e) {
      error = errMsg(e);
    } finally {
      saving = false;
    }
  }

  let abbrEl: HTMLInputElement | undefined = $state();
</script>

<div class="quick">
  <input
    bind:this={abbrEl}
    bind:value={abbr}
    placeholder="/emg"
    spellcheck="false"
    autocapitalize="off"
    onkeydown={(e) => {
      if (e.key === "Enter") expEl?.focus();
    }}
  />
  <span class="arrow">→</span>
  <input
    bind:this={expEl}
    bind:value={expansion}
    placeholder="emeeranjp@gmail.com"
    onkeydown={(e) => {
      if (e.key === "Enter") add();
    }}
  />
  <button class="primary" onclick={add} disabled={saving}>{saving ? "…" : "Add"}</button>
</div>
{#if error}<div class="error">{error}</div>{/if}
<div class="hint faint">
  Tip: start the trigger with <code>/</code> or <code>;</code> so it only fires on purpose —
  typing <code>/emg</code> instantly becomes the expansion. (Autotext must be on.)
</div>

<style>
  .quick {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .quick input:first-of-type {
    width: 130px;
    flex: 0 0 130px;
    font-family: ui-monospace, monospace;
  }
  .arrow {
    color: var(--text-dim);
  }
  .hint {
    font-size: 12px;
    margin-top: 8px;
    line-height: 1.5;
  }
  code {
    background: var(--bg-elev-2);
    padding: 1px 5px;
    border-radius: 4px;
    font-family: ui-monospace, monospace;
  }
</style>
