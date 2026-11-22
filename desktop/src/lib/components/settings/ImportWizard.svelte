<script lang="ts">
  // ImportWizard — file pick → preview (CSV/JSON parsed client-side) → confirm.
  // AHK / aText have no reliable client-side parser: they import directly with a
  // count-confirmation (matching the old flow) rather than a fake preview.
  import { api, errMsg } from "$lib/invoke";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import Modal from "../common/Modal.svelte";
  import type { Folder, Phrase } from "$lib/types";

  type Fmt = "json" | "csv" | "ahk" | "atext" | "textexpander" | "espanso";

  let open = $state(false);
  let fmt = $state<Fmt>("json");
  let fileName = $state("");
  let fileText = $state("");
  let importing = $state(false);
  let targetFolder = $state<string | null>(null);

  interface PreviewRow {
    trigger: string;
    expansion: string;
    folder?: string;
  }
  let previewRows = $state<PreviewRow[]>([]);
  let previewError = $state<string | null>(null);

  const MAX_PREVIEW = 50;

  function openWizard(f: Fmt) {
    fmt = f;
    fileName = "";
    fileText = "";
    previewRows = [];
    previewError = null;
    open = true;
  }

  async function onFilePicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    fileName = file.name;
    fileText = await file.text();
    buildPreview();
  }

  function buildPreview() {
    previewRows = [];
    previewError = null;
    if (fmt === "csv") {
      // CSV: `abbreviation,text` per line (same contract as import_csv).
      const rows: PreviewRow[] = [];
      for (const line of fileText.split(/\r?\n/)) {
        if (!line.trim()) continue;
        const comma = line.indexOf(",");
        if (comma === -1) {
          previewError = "Each line needs `abbreviation,text`.";
          return;
        }
        rows.push({ trigger: line.slice(0, comma).trim(), expansion: line.slice(comma + 1) });
      }
      previewRows = rows;
    } else if (fmt === "json") {
      try {
        const data = JSON.parse(fileText) as { folders?: Folder[]; phrases?: Phrase[] };
        const folderNames = new Map((data.folders ?? []).map((f) => [f.id, f.name]));
        previewRows = (data.phrases ?? []).map((p) => {
          const trig = p.triggers?.[0];
          const trigger = trig?.abbreviation ?? (trig?.key ? `[hotkey] ${trig.key}` : "—");
          const expansion = p.body
            ?.map((seg) => (seg.type === "text" ? seg.text : `{${seg.macro.type}}`))
            .join("");
          return { trigger, expansion, folder: folderNames.get(p.folder_id) };
        });
        if (data.phrases === undefined) previewError = "No `phrases` array found in this JSON.";
      } catch (e) {
        previewError = `Not valid JSON: ${errMsg(e)}`;
      }
    }
    // ahk / atext: no preview — direct import with result toast.
  }

  const hasPreview = $derived(fmt === "csv" || fmt === "json");

  async function doImport() {
    importing = true;
    try {
      let n = 0;
      if (fmt === "json") n = await api.importJson(fileText, targetFolder);
      else if (fmt === "csv") n = await api.importCsv(fileText, targetFolder);
      else if (fmt === "ahk") n = await api.importAhk(fileText, targetFolder);
      else if (fmt === "textexpander") n = await api.importTextexpander(fileText, targetFolder);
      else if (fmt === "espanso") n = await api.importEspanso(fileText, targetFolder);
      else n = await api.importAtext(fileText, targetFolder);
      toast(`Imported ${n} phrase${n === 1 ? "" : "s"}`, "ok");
      open = false;
      await Promise.all([foldersStore.load(), phrasesStore.load()]);
    } catch (e) {
      toast(errMsg(e), "err");
    } finally {
      importing = false;
    }
  }
</script>

<button onclick={() => openWizard("json")}>⭱ PhraseXp JSON</button>
<button onclick={() => openWizard("csv")}>⭱ CSV</button>
<button onclick={() => openWizard("ahk")}>⭱ AutoHotKey</button>
<button onclick={() => openWizard("atext")}>⭱ aText</button>
<button onclick={() => openWizard("textexpander")}>⭱ TextExpander</button>
<button onclick={() => openWizard("espanso")}>⭱ Espanso</button>
<input type="file" style="display:none" onchange={onFilePicked}
  accept={fmt === "json" ? ".json"
    : fmt === "csv" ? ".csv,text/csv"
    : fmt === "ahk" ? ".ahk,.txt"
    : fmt === "textexpander" ? ".csv,text/csv"
    : fmt === "espanso" ? ".yml,.yaml,.txt"
    : ".atext,.json"} />

<Modal {open} title="Import phrases" onclose={() => (open = false)}>
  <div class="wizard">
    <div class="row between">
      <strong>Import — {fmt.toUpperCase()}{fileName ? ` · ${fileName}` : ""}</strong>
      <span class="faint">{hasPreview ? `${previewRows.length} rows` : "no preview for this format"}</span>
    </div>

    {#if !fileName}
      <p class="muted" style="margin:8px 0">
        {#if fmt === "json"}PhraseXp export — preserves folders + macros.{/if}
        {#if fmt === "csv"}One <code>abbreviation,text</code> pair per line.{/if}
        {#if fmt === "ahk"}AutoHotKey hotstrings: <code>::trigger::replacement</code>. Imported directly (no preview).{/if}
        {#if fmt === "atext"}aText backup (binary or JSON; auto-detected). Imported directly (no preview).{/if}
        {#if fmt === "textexpander"}TextExpander CSV export — <code>abbreviation,snippet,label</code> columns. Imported directly (no preview).{/if}
        {#if fmt === "espanso"}Espanso matches YAML — literal <code>trigger</code>/<code>replace</code> pairs; forms and script vars are skipped. Imported directly (no preview).{/if}
      </p>
      <button class="primary" onclick={() => document.querySelector<HTMLInputElement>(".wizard input[type=file]")?.click()}>
        Choose file…
      </button>
    {:else}
      <div class="field">
        <label for="imp-folder">Target folder <span class="faint">(optional — default per format)</span></label>
        <select id="imp-folder" bind:value={targetFolder}>
          <option value={null}>Default ("Imported")</option>
          {#each foldersStore.items as f (f.id)}
            <option value={f.id}>{f.name}</option>
          {/each}
        </select>
      </div>

      {#if previewError}
        <div class="error">{previewError}</div>
      {:else if hasPreview}
        <div class="preview scroll">
          <table>
            <thead><tr><th>Trigger</th><th>Expansion</th>{#if fmt === "json"}<th>Folder</th>{/if}</tr></thead>
            <tbody>
              {#each previewRows.slice(0, MAX_PREVIEW) as r, i (i)}
                <tr>
                  <td><code>{r.trigger}</code></td>
                  <td>{r.expansion.slice(0, 80)}{r.expansion.length > 80 ? "…" : ""}</td>
                  {#if fmt === "json"}<td class="faint">{r.folder ?? "—"}</td>{/if}
                </tr>
              {/each}
            </tbody>
          </table>
          {#if previewRows.length > MAX_PREVIEW}
            <p class="faint">…and {previewRows.length - MAX_PREVIEW} more</p>
          {/if}
        </div>
      {/if}

      <div class="row" style="justify-content:flex-end; gap:8px">
        <button class="ghost" onclick={() => { fileName = ""; fileText = ""; previewRows = []; previewError = null; }}>
          Pick another
        </button>
        <button class="primary" onclick={doImport} disabled={importing || (!!previewError && hasPreview)}>
          {importing ? "Importing…" : `Import ${previewRows.length > 0 ? previewRows.length + " phrases" : "file"}`}
        </button>
      </div>
    {/if}
  </div>
</Modal>

<style>
  .wizard {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 420px;
  }
  .preview {
    max-height: 300px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-sm);
  }
  th {
    text-align: left;
    color: var(--text-dim);
    padding: 6px 10px;
    border-bottom: 1px solid var(--border-soft);
    font-size: var(--fs-xs);
  }
  td {
    padding: 5px 10px;
    border-bottom: 1px solid var(--border-soft);
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  code {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    background: var(--surface-2);
    padding: 1px 5px;
    border-radius: var(--radius-xs);
  }
</style>
