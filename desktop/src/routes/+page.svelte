<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getTheme, toggleTheme } from "$lib/theme";
  import PhraseEditor from "$lib/components/PhraseEditor.svelte";
  import Popup from "$lib/components/Popup.svelte";
  import SettingsPanel from "$lib/components/Settings.svelte";
  import QuickAdd from "$lib/components/QuickAdd.svelte";
  import { api, errMsg } from "$lib/invoke";
  import type {
    AutotextStatus,
    ClipboardEntry,
    Folder,
    Phrase,
    PhraseSummary,
    PhraseVersion,
    SortKey,
  } from "$lib/types";

  // The popup window shares this SPA entry; branch on its label.
  const isPopup = getCurrentWindow().label === "popup";

  // data
  let folders = $state<Folder[]>([]);
  let phrases = $state<PhraseSummary[]>([]);
  let clipboard = $state<ClipboardEntry[]>([]);

  // filters / selection
  let selectedFolderId = $state<string | null>(null);
  let query = $state("");
  let sort = $state<SortKey>("name");

  // editor
  let editing = $state<{ phrase: Phrase | null; folderId: string } | null>(null);
  let showSettings = $state(false);
  let showDupes = $state(false);
  let dupGroups = $state<PhraseSummary[][]>([]);
  let showHistory = $state(false);
  let historyPhrase = $state<PhraseSummary | null>(null);
  let versions = $state<PhraseVersion[]>([]);
  let theme = $state(getTheme());
  let folderCounts = $state<Map<string, number>>(new Map());

  // expander
  let expanderType = $state<"autotext" | "hotkey">("autotext");
  let expanderValue = $state("br,");
  let expandOutput = $state("");
  let expandError = $state<string | null>(null);

  // misc
  let newFolderName = $state("");
  let newClipText = $state("");
  let banner = $state<string | null>(null);

  async function refreshFolders() {
    folders = await api.listFolders();
    const counts = await api.folderCounts();
    folderCounts = new Map(counts.map(([id, n]) => [id, n] as [string, number]));
  }
  async function refreshPhrases() {
    phrases = await api.listPhrases(selectedFolderId, query.trim() || undefined, false, sort);
  }
  async function refreshClipboard() {
    clipboard = await api.listClipboard();
  }

  onMount(async () => {
    try {
      await Promise.all([refreshFolders(), refreshClipboard()]);
    } catch (e) {
      banner = errMsg(e);
    }
  });

  // reload phrases whenever the folder filter, search, or sort changes
  $effect(() => {
    void selectedFolderId;
    void query;
    void sort;
    refreshPhrases().catch((e) => (banner = errMsg(e)));
  });

  // M4: live autotext-listener health (poll every 2s).
  let autotext = $state<AutotextStatus | null>(null);
  $effect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        if (!cancelled) autotext = await api.autotextStatus();
      } catch {
        /* ignore until next tick */
      }
    };
    void tick();
    const id = setInterval(tick, 2000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  });

  async function addFolder() {
    if (!newFolderName.trim()) return;
    try {
      await api.createFolder({ name: newFolderName.trim() });
      newFolderName = "";
      await refreshFolders();
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function removeFolder(f: Folder) {
    if (!confirm(`Delete folder "${f.name}"?`)) return;
    try {
      await api.deleteFolder(f.id);
      if (selectedFolderId === f.id) selectedFolderId = null;
      await refreshFolders();
    } catch (e) {
      banner = errMsg(e);
    }
  }

  function selectFolder(id: string | null) {
    selectedFolderId = id;
  }

  function newPhrase() {
    const folderId = selectedFolderId ?? folders[0]?.id;
    if (!folderId) {
      banner = "Create a folder first.";
      return;
    }
    editing = { phrase: null, folderId };
  }

  async function editPhrase(p: PhraseSummary) {
    try {
      const full = await api.getPhrase(p.id);
      editing = { phrase: full, folderId: full.folder_id };
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function onSaved() {
    editing = null;
    await refreshPhrases();
  }

  async function removePhrase(p: PhraseSummary) {
    if (!confirm(`Delete phrase "${p.name}"?`)) return;
    try {
      await api.deletePhrase(p.id);
      await refreshPhrases();
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function doExpand() {
    expandError = null;
    expandOutput = "";
    try {
      const res = await api.expand(expanderType, expanderValue.trim());
      expandOutput = res.output;
    } catch (e) {
      expandError = errMsg(e);
    }
  }

  async function addClip() {
    if (!newClipText.trim()) return;
    try {
      await api.addClipboard(newClipText.trim());
      newClipText = "";
      await refreshClipboard();
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function clearClipboard() {
    try {
      await api.clearClipboard();
      await refreshClipboard();
    } catch (e) {
      banner = errMsg(e);
    }
  }

  let csvInput: HTMLInputElement | undefined = $state();
  let ahkInput: HTMLInputElement | undefined = $state();

  async function copyPhrase(p: PhraseSummary) {
    try {
      await api.injectPhrase(p.id, {}, "copy");
      banner = `Copied “${p.name}” to clipboard`;
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function exportLib() {
    try {
      const data = await api.exportLibrary();
      const blob = new Blob([JSON.stringify(data, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "phrasexp-library.json";
      a.click();
      URL.revokeObjectURL(url);
      banner = "Library exported to phrasexp-library.json";
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function onCsvPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    try {
      const n = await api.importCsv(text, selectedFolderId);
      banner = `Imported ${n} phrase${n === 1 ? "" : "s"}`;
      await Promise.all([refreshFolders(), refreshPhrases()]);
    } catch (err) {
      banner = errMsg(err);
    }
    input.value = "";
  }

  async function onAhkPicked(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    try {
      const n = await api.importAhk(text, selectedFolderId);
      banner = `Imported ${n} phrase${n === 1 ? "" : "s"} from AHK`;
      await Promise.all([refreshFolders(), refreshPhrases()]);
    } catch (err) {
      banner = errMsg(err);
    }
    input.value = "";
  }

  async function loadDupes() {
    try {
      dupGroups = await api.findDuplicates();
      showDupes = true;
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function loadHistory(p: PhraseSummary) {
    try {
      versions = await api.listVersions(p.id);
      historyPhrase = p;
      showHistory = true;
    } catch (e) {
      banner = errMsg(e);
    }
  }

  async function doRestore(versionId: string) {
    if (!historyPhrase) return;
    try {
      await api.restoreVersion(historyPhrase.id, versionId);
      banner = "Restored previous version";
      showHistory = false;
      await refreshPhrases();
    } catch (e) {
      banner = errMsg(e);
    }
  }
</script>

{#if isPopup}
  <Popup />
{:else}
<header>
  <div class="row between">
    <h1>phrasexp</h1>
    <div class="row" style="gap:10px">
      <span class="muted">{phrases.length} phrases · {folders.length} folders · Ctrl+Alt+Space = popup</span>
      {#if autotext}
        <span
          class="tag {autotext.enabled && autotext.running ? '' : 'warn'}"
          title={autotext.enabled && !autotext.running
            ? "Listener not receiving keys. On Linux: add yourself to the 'input' group and re-login (`sudo usermod -aG input $USER`). On macOS: grant Accessibility."
            : `autotext: ${autotext.enabled ? 'enabled' : 'disabled'}`}
        >
          autotext {autotext.enabled ? (autotext.running ? "live" : "blocked") : "off"} · {autotext.events_seen} evts
        </span>
      {/if}
      <button class="ghost tiny" onclick={loadDupes} title="Find duplicate phrases">⧉ Dupes</button>
      <button class="ghost tiny" title="Toggle light/dark theme" onclick={() => (theme = toggleTheme())}>
        {theme === "dark" ? "☀" : "☾"}
      </button>
      <button class="ghost tiny" title="Settings" onclick={() => (showSettings = true)}>⚙</button>
    </div>
  </div>
  {#if banner}<div class="error">{banner}</div>{/if}
</header>

<main class="grid">
  <!-- Sidebar: folders + clipboard -->
  <aside class="pane scroll">
    <div class="col" style="gap:6px">
      <strong>Folders</strong>
      <button class="row between row-btn" class:on={selectedFolderId === null} onclick={() => selectFolder(null)}>
        <span>All phrases</span>
      </button>
      {#each folders as f (f.id)}
        <div class="row between row-btn" class:on={selectedFolderId === f.id}>
          <button class="ghost" style="flex:1; text-align:left; justify-content:flex-start" onclick={() => selectFolder(f.id)}>
            📁 {f.name} <span class="faint">({folderCounts.get(f.id) ?? 0})</span>
          </button>
          <button class="tiny danger ghost" onclick={() => removeFolder(f)}>✕</button>
        </div>
      {/each}
      <div class="row" style="margin-top:4px">
        <input bind:value={newFolderName} placeholder="New folder…" onkeydown={(e) => e.key === "Enter" && addFolder()} />
        <button onclick={addFolder}>+</button>
      </div>
    </div>

    <div class="divider" style="margin:16px 0"></div>

    <div class="col" style="gap:6px">
      <div class="row between">
        <strong>Clipboard</strong>
        {#if clipboard.length > 0}
          <button class="tiny danger ghost" onclick={clearClipboard}>Clear</button>
        {/if}
      </div>
      <div class="row">
        <input bind:value={newClipText} placeholder="Capture text…" onkeydown={(e) => e.key === "Enter" && addClip()} />
        <button onclick={addClip}>+</button>
      </div>
      <div class="clip-list">
        {#each clipboard as c (c.id)}
          <div class="clip" title={c.captured_at}>{c.text}</div>
        {/each}
        {#if clipboard.length === 0}<div class="faint">No entries yet.</div>{/if}
      </div>
    </div>
  </aside>

  <!-- Center: phrase list -->
  <section class="pane scroll">
    <QuickAdd
      folders={folders}
      selectedFolderId={selectedFolderId}
      onadded={() => {
        refreshFolders();
        refreshPhrases();
      }}
    />
    <div class="divider" style="margin:12px 0"></div>
    <div class="row between" style="margin-bottom:10px">
      <input bind:value={query} placeholder="Search phrases…" style="flex:1" />
      <select bind:value={sort} style="flex:0 0 110px" aria-label="Sort phrases">
        <option value="name">Name</option>
        <option value="updated">Recently edited</option>
        <option value="created">Newest first</option>
      </select>
      <button class="primary" onclick={newPhrase}>+ New phrase</button>
    </div>
    {#each phrases as p (p.id)}
      <div class="phrase" role="button" tabindex="0" onclick={() => editPhrase(p)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && editPhrase(p)}>
        <div class="row between">
          <strong>{p.name}</strong>
          <div class="row" style="gap:6px">
            {#if p.abbreviation}<span class="tag">{p.abbreviation}</span>{/if}
            {#if p.sensitive}<span class="tag warn">sensitive</span>{/if}
          </div>
        </div>
        <div class="preview muted">{p.content_preview}</div>
        <div class="row" style="justify-content:flex-end; margin-top:6px">
          <button class="tiny ghost" onclick={(e) => { e.stopPropagation(); copyPhrase(p); }}>Copy</button>
          <button class="tiny ghost" onclick={(e) => { e.stopPropagation(); loadHistory(p); }}>History</button>
          <button class="tiny danger ghost" onclick={(e) => { e.stopPropagation(); removePhrase(p); }}>Delete</button>
        </div>
      </div>
    {/each}
    {#if phrases.length === 0}<div class="faint">No phrases. Create one with “+ New phrase”.</div>{/if}
  </section>

  <!-- Right: editor or expander -->
  <aside class="pane scroll">
    {#if editing}
      <PhraseEditor
        phrase={editing.phrase}
        folderId={editing.folderId}
        folders={folders}
        phrases={phrases}
        onsaved={onSaved}
        oncancel={() => (editing = null)}
      />
    {:else}
      <div class="col" style="gap:12px">
        <h3 style="margin:0">Test expansion</h3>
        <p class="muted" style="margin:0">
          Resolves a trigger against the library and returns the expanded text (delivery = OS injection arrives in M3).
        </p>
        <div class="row">
          <select bind:value={expanderType} style="flex:0 0 120px">
            <option value="autotext">autotext</option>
            <option value="hotkey">hotkey</option>
          </select>
          <input bind:value={expanderValue} placeholder={expanderType === "autotext" ? "br," : "ctrl+shift+b"} />
          <button class="primary" onclick={doExpand}>Expand</button>
        </div>
        {#if expandOutput}
          <div class="output">{expandOutput}</div>
        {/if}
        {#if expandError}<div class="error">{expandError}</div>{/if}
      </div>
    {/if}
  </aside>
</main>

{#if showSettings}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="modal-overlay"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) showSettings = false; }}
    onkeydown={(e) => e.key === "Escape" && (showSettings = false)}
  >
    <div class="modal" role="dialog" aria-modal="true">
      <div class="row" style="justify-content:flex-end">
        <button class="ghost tiny" onclick={() => (showSettings = false)}>✕ close</button>
      </div>
      <SettingsPanel />
    </div>
  </div>
{/if}

{#if showDupes}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="modal-overlay"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) showDupes = false; }}
    onkeydown={(e) => e.key === "Escape" && (showDupes = false)}
  >
    <div class="modal" role="dialog" aria-modal="true">
      <div class="row between">
        <strong>Duplicate phrases ({dupGroups.length} groups)</strong>
        <button class="ghost tiny" onclick={() => (showDupes = false)}>✕ close</button>
      </div>
      {#if dupGroups.length === 0}
        <div class="faint" style="margin-top:8px">No duplicates found.</div>
      {/if}
      {#each dupGroups as group, gi (gi)}
        <div class="dup-group">
          {#each group as p (p.id)}
            <div class="dup-item"><strong>{p.name}</strong> <span class="faint">— {p.content_preview}</span></div>
          {/each}
        </div>
      {/each}
    </div>
  </div>
{/if}

{#if showHistory}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="modal-overlay"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) showHistory = false; }}
    onkeydown={(e) => e.key === "Escape" && (showHistory = false)}
  >
    <div class="modal" role="dialog" aria-modal="true">
      <div class="row between">
        <strong>History — {historyPhrase?.name}</strong>
        <button class="ghost tiny" onclick={() => (showHistory = false)}>✕ close</button>
      </div>
      {#if versions.length === 0}
        <div class="faint" style="margin-top:8px">No previous versions yet.</div>
      {/if}
      {#each versions as v (v.id)}
        <div class="dup-group">
          <div class="row between">
            <span>
              {#if v.preview}{v.preview}{:else}<span class="faint">(empty)</span>{/if}
              <br /><span class="faint">{v.saved_at}</span>
            </span>
            <button class="tiny" onclick={() => doRestore(v.id)}>Restore</button>
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}
{/if}

<style>
  header {
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-soft);
  }
  header h1 {
    margin: 0;
    font-size: 18px;
    letter-spacing: -0.01em;
  }
  .grid {
    display: grid;
    grid-template-columns: 230px minmax(0, 1fr) 380px;
    gap: 1px;
    background: var(--border-soft);
    height: calc(100vh - 53px);
  }
  .pane {
    background: var(--bg);
    padding: 14px;
    min-width: 0;
  }
  aside.pane {
    background: var(--bg-elev);
  }
  .row-btn {
    border-radius: var(--radius-sm);
    padding: 2px;
  }
  .row-btn :global(button) {
    justify-content: flex-start;
  }
  .row-btn.on {
    background: var(--accent-soft);
  }
  .clip-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 6px;
  }
  .clip {
    background: var(--bg);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 5px 8px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .phrase {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--bg-elev);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    margin-bottom: 4px;
  }
  .phrase:hover {
    border-color: var(--accent);
  }
  .preview {
    margin-top: 2px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .output {
    background: var(--bg);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    padding: 10px;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .modal {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px 20px;
    width: 520px;
    max-width: 90vw;
    max-height: 85vh;
    overflow-y: auto;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.4);
  }
  .dup-group {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border-soft);
  }
  .dup-item {
    padding: 5px 0;
    font-size: 13px;
  }
</style>
