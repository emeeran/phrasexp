<script lang="ts">
  // AppShell — sidebar + header + page + status bar. Owns initial data
  // loads and global event listeners. The page area renders the legacy
  // 3-column markup (moved here from +page.svelte) but fed by stores.
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { app } from "$lib/stores/app.svelte";
  import { foldersStore } from "$lib/stores/folders.svelte";
  import { phrasesStore } from "$lib/stores/phrases.svelte";
  import { clipboardStore } from "$lib/stores/clipboard.svelte";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { statusStore } from "$lib/stores/status.svelte";
  import { toast } from "$lib/stores/toast.svelte";
  import { errMsg, api } from "$lib/invoke";
  import { fmtDateTime } from "$lib/dates";
  import Sidebar from "./Sidebar.svelte";
  import Header from "./Header.svelte";
  import StatusBar from "./StatusBar.svelte";
  import Toast from "../common/Toast.svelte";
  import LibraryView from "../library/LibraryView.svelte";
  import ClipboardPage from "../clipboard/ClipboardPage.svelte";
  import MacrosPage from "../macros/MacrosPage.svelte";
  import AiPage from "../ai/AiPage.svelte";
  import SettingsPage from "../settings/SettingsPage.svelte";
  import Modal from "../common/Modal.svelte";
  import Icon from "../common/Icon.svelte";

  async function doRestore(versionId: string) {
    if (!app.historyPhrase) return;
    try {
      await api.restoreVersion(app.historyPhrase.id, versionId);
      toast("Restored previous version", "ok");
      app.modal = null;
      await phrasesStore.load();
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  // Delete one phrase from the dupes modal, then re-scan so emptied groups disappear.
  async function deleteDupe(phraseId: string, name: string) {
    if (!confirm(`Delete phrase "${name}"?`)) return;
    try {
      await api.deletePhrase(phraseId);
      toast(`Phrase "${name}" deleted`, "ok");
      app.dupGroups = (await api.findDuplicates())
        .filter((g) => g.some((p) => p.id !== phraseId));
      await Promise.all([phrasesStore.load(), foldersStore.load()]);
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  // Bulk cleanup: delete every phrase except the first-listed of each group.
  let dupesBusy = $state(false);

  async function deleteDupesBatch(doomed: import("$lib/types").PhraseSummary[]) {
    if (doomed.length === 0 || dupesBusy) return;
    if (
      !confirm(
        `Delete ${doomed.length} duplicate phrase${doomed.length === 1 ? "" : "s"}?\n\n` +
          `The first-listed phrase in each group is kept; the rest are removed.`,
      )
    )
      return;
    dupesBusy = true;
    let n = 0;
    try {
      for (const p of doomed) {
        await api.deletePhrase(p.id);
        n++;
      }
      toast(`Deleted ${n} duplicate phrase${n === 1 ? "" : "s"}`, "ok");
    } catch (e) {
      toast(`${errMsg(e)} — stopped after ${n} deletion${n === 1 ? "" : "s"}`, "err");
    } finally {
      // Re-scan + refresh regardless of outcome so the UI reflects reality.
      app.dupGroups = await api.findDuplicates();
      await Promise.all([phrasesStore.load(), foldersStore.load()]);
      dupesBusy = false;
    }
  }

  /** All groups at once: keep the first of each group, delete the rest. */
  function deleteAllDupes() {
    void deleteDupesBatch(app.dupGroups.flatMap((g) => g.slice(1)));
  }

  /** One group: keep its first-listed phrase, delete the others. */
  function deleteGroupDupes(group: import("$lib/types").PhraseSummary[]) {
    void deleteDupesBatch(group.slice(1));
  }

  const doomedCount = $derived(
    app.dupGroups.reduce((n, g) => n + Math.max(0, g.length - 1), 0),
  );

  onMount(() => {
    // Global listeners first, then data.
    listen<string>("phrase-error", (e) => toast(`⚠ ${e.payload}`, "err"));
    listen<string>("spellcheck-learned", (e) =>
      toast(`Spellcheck will now ignore “${e.payload}”`, "info"),
    );
    listen<boolean>("spellcheck-toggled", (e) =>
      toast(e.payload ? "Spellcheck ON" : "Spellcheck OFF", "info"),
    );
    statusStore.start();
    Promise.all([
      foldersStore.load(),
      clipboardStore.load(),
      settingsStore.load(),
      phrasesStore.load(),
    ]).catch((e) => toast(errMsg(e), "err"));
  });
</script>

<div class="shell">
  <Sidebar />
  <div class="main">
    <Header />
    <main class="page">
      {#if app.route === "library"}
        <LibraryView />
      {:else if app.route === "clipboard"}
        <ClipboardPage />
      {:else if app.route === "macros"}
        <MacrosPage />
      {:else if app.route === "ai"}
        <AiPage />
      {:else if app.route === "settings"}
        <SettingsPage />
      {/if}
    </main>
    <StatusBar />
  </div>
</div>

{#if app.modal === "dupes"}
  <Modal open title={`Duplicate phrases (${app.dupGroups.length} groups)`} onclose={() => (app.modal = null)}>
    {#if doomedCount > 0}
      <div class="row modal-head" style="justify-content:flex-end">
        <button class="tiny danger" disabled={dupesBusy} onclick={deleteAllDupes}>
          {dupesBusy ? "Deleting…" : `🗑 Delete all but first (${doomedCount})`}
        </button>
      </div>
    {/if}
    {#if app.dupGroups.length === 0}
      <div class="faint" style="margin-top:8px">No duplicates found.</div>
    {/if}
    {#each app.dupGroups as group, gi (gi)}
      <div class="dup-group">
        <div class="row between" style="margin-bottom:2px">
          <span class="tag muted">group {gi + 1} · {group.length} copies</span>
          {#if group.length > 1}
            <button class="tiny danger ghost" disabled={dupesBusy} onclick={() => deleteGroupDupes(group)}>
              keep first, delete {group.length - 1}
            </button>
          {/if}
        </div>
        {#each group as p, pi (p.id)}
          <div class="dup-item" class:keep={pi === 0}>
            <span class="dup-text">
              {#if pi === 0}<span class="tag" style="margin-right:6px">keep</span>{/if}
              <strong>{p.name}</strong> <span class="faint">— {p.content_preview}</span>
            </span>
            <button class="tiny danger ghost" title="Delete this phrase" disabled={dupesBusy} onclick={() => deleteDupe(p.id, p.name)}>
              <Icon name="trash" size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/each}
  </Modal>
{/if}

{#if app.modal === "history"}
  <Modal open title={`History — ${app.historyPhrase?.name ?? ""}`} onclose={() => (app.modal = null)}>
    {#if app.versions.length === 0}
      <div class="faint" style="margin-top:8px">No previous versions yet.</div>
    {/if}
    {#each app.versions as v (v.id)}
      <div class="dup-group">
        <div class="row between">
          <span>
            {#if v.preview}{v.preview}{:else}<span class="faint">(empty)</span>{/if}
            <br /><span class="faint">{fmtDateTime(v.saved_at)}</span>
          </span>
          <button class="tiny" onclick={() => doRestore(v.id)}>Restore</button>
        </div>
      </div>
    {/each}
  </Modal>
{/if}

<Toast />

<style>
  .shell {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }
  .main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .page {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .modal-head {
    margin-bottom: 6px;
  }
  .dup-group {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border-soft);
  }
  .dup-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 5px 0;
    font-size: var(--fs-base);
  }
  .dup-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dup-item.keep .dup-text strong {
    color: var(--ok);
  }
</style>
