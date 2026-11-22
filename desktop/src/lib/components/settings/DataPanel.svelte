<script lang="ts">
  // Data panel — export, import wizard, top phrases, danger zone.
  import { onMount } from "svelte";
  import { api, errMsg } from "$lib/invoke";
  import { app } from "$lib/stores/app.svelte";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import type { PhraseSummary } from "$lib/types";
  import ImportWizard from "./ImportWizard.svelte";

  async function exportLib() {
    try {
      const data = await api.exportLibrary();
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      downloadBlob(blob, "phrasexp-library.json");
      const nSensitive = data.sensitive_count ?? 0;
      if (nSensitive > 0) {
        toast(
          `Exported — includes ${nSensitive} sensitive phrase${nSensitive === 1 ? "" : "s"} in PLAINTEXT; treat the file as secret`,
          "warn",
        );
      } else {
        toast("Library exported", "ok");
      }
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  function downloadBlob(blob: Blob, filename: string) {
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }

  /** Format exporters: Rust formats the text-only snippets; macros, images, and
   *  regex triggers can't transfer, so the toast reports what was skipped. */
  async function exportFormat(
    kind: "espanso" | "textexpander" | "csv",
  ) {
    try {
      const r =
        kind === "espanso"
          ? await api.exportEspanso()
          : kind === "textexpander"
            ? await api.exportTextexpanderCsv()
            : await api.exportCsv();
      const file = {
        espanso: { blob: new Blob([r.text], { type: "text/yaml" }), name: "espanso-matches.yaml" },
        textexpander: { blob: new Blob([r.text], { type: "text/csv" }), name: "textexpander-snippets.csv" },
        csv: { blob: new Blob([r.text], { type: "text/csv" }), name: "phrasexp-snippets.csv" },
      }[kind];
      downloadBlob(file.blob, file.name);
      toast(
        `Exported ${r.exported} snippet${r.exported === 1 ? "" : "s"}${
          r.skipped > 0 ? ` — ${r.skipped} skipped (macros/images/regex don't transfer)` : ""
        }`,
        "ok",
      );
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  async function doReset() {
    if (!confirm("Delete ALL phrases, folders, clipboard, and version history? This cannot be undone.")) return;
    try {
      await api.resetDatabase();
      toast("Database reset — restart PhraseXp", "warn");
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  async function findDupes() {
    try {
      app.dupGroups = await api.findDuplicates();
      app.modal = "dupes";
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  // Usage stats (3.2): never-expanded phrases — cleanup candidates. Loaded on
  // demand ("Review unused") so the panel stays cheap.
  let unusedCount = $state<number | null>(null);
  async function loadUnusedCount() {
    try {
      const all = await api.listPhrases(undefined, undefined, false, "name");
      unusedCount = all.filter((p) => (p.expanded_count ?? 0) === 0).length;
    } catch {
      // stats are decorative — ignore load failures
    }
  }
  async function reviewUnused() {
    try {
      const all = await api.listPhrases(undefined, undefined, false, "name");
      app.unusedPhrases = all.filter((p) => (p.expanded_count ?? 0) === 0);
      app.modal = "unused";
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  // Usage stats (1.4): top phrases by expansion count, loaded on mount.
  let topPhrases = $state<PhraseSummary[]>([]);
  // File-based sync state: the configured folder, the snapshot header, and busy
  // flag for the "Sync now" button.
  let syncDir = $state("");
  let syncInfo = $state<{ device: string | null; exported_at: string | null; phrases: number | null } | null>(null);
  let syncing = $state(false);
  onMount(async () => {
    try {
      const all = await api.listPhrases(undefined, undefined, false, "most_used");
      topPhrases = all.filter((p) => (p.expanded_count ?? 0) > 0).slice(0, 5);
      unusedCount = all.filter((p) => (p.expanded_count ?? 0) === 0).length;
    } catch {
      // stats are decorative — ignore load failures
    }
    try {
      const st = await api.syncStatus();
      syncDir = st.dir;
      syncInfo = st.dir.trim()
        ? { device: st.device, exported_at: st.exported_at, phrases: st.phrases }
        : null;
    } catch {
      // sync status is optional — ignore
    }
  });

  async function saveSyncDir() {
    try {
      await api.syncSetDir(syncDir);
      syncInfo = syncDir.trim()
        ? { device: null, exported_at: null, phrases: null }
        : null;
      toast(syncDir.trim() ? "Sync folder saved" : "Sync disabled", "ok");
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  // Snapshot passphrase (3.1): seals library.json with AES-256-GCM under a
  // PBKDF2 key. Same passphrase required on every device sharing the folder.
  let syncPassphrase = $state("");
  async function saveSyncPassphrase() {
    try {
      await api.syncSetPassphrase(syncPassphrase);
      toast(
        syncPassphrase
          ? "Snapshot encryption enabled — use the SAME passphrase on every device"
          : "Snapshot encryption off — the next sync writes plaintext",
        syncPassphrase ? "ok" : "warn",
      );
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  async function doSyncNow() {
    if (syncing) return;
    syncing = true;
    try {
      const r = await api.syncNow();
      syncInfo = {
        device: null,
        exported_at: new Date().toISOString(),
        phrases: r.stored,
      };
      await phrasesStore.load();
      await foldersStore.load();
      toast(`Synced — ${r.imported} imported, ${r.updated} updated, ${r.deleted} removed`, "ok");
    } catch (e) {
      toast(errMsg(e), "err");
    } finally {
      syncing = false;
    }
  }
</script>

<div class="col">
  <section class="card">
    <div class="card-h">Backup</div>
    <p class="faint">Export your full library (folders, phrases, macros) to a JSON file you can re-import anywhere.</p>
    <button onclick={exportLib}>⭳ Export library as JSON</button>
    <p class="faint">Migrating to another tool? Text-only snippets export to Espanso or TextExpander (macros, images, and regex triggers don't transfer).</p>
    <div class="export-row">
      <button onclick={() => exportFormat("espanso")}>Espanso YAML</button>
      <button onclick={() => exportFormat("textexpander")}>TextExpander CSV</button>
      <button onclick={() => exportFormat("csv")}>Plain CSV</button>
    </div>
  </section>

  <section class="card">
    <div class="card-h">Sync</div>
    <p class="faint">
      Mirror your library to a folder you own — Syncthing, WebDAV, a git repo, or a USB
      drive keep it in step across devices. Merges per-phrase last-writer-wins; deletions
      propagate. Settings, counters, and clipboard history stay local.
    </p>
    <div class="sync-row">
      <input
        class="sync-dir"
        bind:value={syncDir}
        placeholder="/path/to/sync/folder — empty = off"
        spellcheck="false"
      />
      <button onclick={saveSyncDir}>Save</button>
    </div>
    <div class="sync-row">
      <button disabled={syncing || !syncDir.trim()} onclick={doSyncNow}>
        {syncing ? "Syncing…" : "⇅ Sync now"}
      </button>
      {#if syncInfo}
        <span class="faint sync-meta">
          {#if syncInfo.exported_at}
            snapshot: {syncInfo.phrases ?? "?"} phrases
            {#if syncInfo.device}, from {syncInfo.device}{/if}
            — {new Date(syncInfo.exported_at).toLocaleString()}
          {:else}
            not synced yet
          {/if}
        </span>
      {/if}
    </div>
    <div class="sync-row">
      <input
        class="sync-dir"
        type="password"
        bind:value={syncPassphrase}
        placeholder="Passphrase — empty = plaintext snapshot"
        aria-label="Sync snapshot passphrase"
      />
      <button onclick={saveSyncPassphrase}>Seal</button>
    </div>
    <p class="faint">
      With a passphrase, the snapshot is encrypted (AES-256-GCM) — use the SAME
      passphrase on every device sharing the folder.
    </p>
    <p class="faint warn-line">Without one, the snapshot file is plaintext — sensitive phrases are included unencrypted.</p>
  </section>

  <section class="card">
    <div class="card-h">Import</div>
    <ImportWizard />
    <p class="faint">PhraseXp JSON preserves folders + macros. CSV / AutoHotKey / aText create simple phrases in an "Imported" folder.</p>
  </section>

  <section class="card">
    <div class="card-h">Usage stats</div>
    {#if topPhrases.length === 0}
      <p class="faint">No expansions counted yet — usage builds up as you type abbreviations.</p>
    {:else}
      <div class="col" style="gap:4px">
        {#each topPhrases as p, i (p.id)}
          <div class="top-row">
            <span class="top-rank">{i + 1}</span>
            <span class="top-name">{p.name}</span>
            <span class="tag mono">×{p.expanded_count}</span>
          </div>
        {/each}
      </div>
    {/if}
    {#if unusedCount !== null && unusedCount > 0}
      <button onclick={reviewUnused}>Review {unusedCount} never-used phrase{unusedCount === 1 ? "" : "s"}</button>
    {:else if unusedCount === 0}
      <p class="faint">Every phrase has been used at least once.</p>
    {/if}
  </section>

  <section class="card">
    <div class="card-h">Duplicates</div>
    <p class="faint">Scan for phrases with identical first-text content, review the groups, and delete the extras from the results.</p>
    <button onclick={findDupes}>⧉ Find duplicates</button>
  </section>

  <section class="card danger">
    <div class="card-h">Danger zone</div>
    <p class="faint">Permanently delete ALL phrases, folders, clipboard, and history.</p>
    <button class="danger" onclick={doReset}>🗑 Reset database</button>
  </section>
</div>

<style>
  .col {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    align-items: start;
    gap: 10px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
  .card-h {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: var(--fs-md);
  }
  .card.danger {
    border-color: var(--danger);
    background: var(--danger-soft);
  }
  .export-row {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .top-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 0;
    font-size: var(--fs-base);
  }
  .top-rank {
    width: 18px;
    color: var(--text-faint);
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    text-align: right;
  }
  .top-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sync-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .sync-dir {
    flex: 1;
    padding: 6px 8px;
    font-size: var(--fs-base);
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    color: var(--text);
  }
  .sync-dir:focus {
    outline: none;
    border-color: var(--accent);
  }
  .sync-meta {
    font-size: var(--fs-xs);
  }
  .warn-line {
    margin-top: 2px;
  }
</style>
