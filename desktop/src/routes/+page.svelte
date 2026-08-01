<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import PhraseEditor from "$lib/components/PhraseEditor.svelte";
  import Popup from "$lib/components/Popup.svelte";
  import SettingsPanel from "$lib/components/Settings.svelte";
  import { api, errMsg } from "$lib/invoke";
  import type { AutotextStatus, ClipboardEntry, Folder, Phrase, PhraseSummary } from "$lib/types";

  // The popup window shares this SPA entry; branch on its label.
  const isPopup = getCurrentWindow().label === "popup";

  // data
  let folders = $state<Folder[]>([]);
  let phrases = $state<PhraseSummary[]>([]);
  let clipboard = $state<ClipboardEntry[]>([]);

  // filters / selection
  let selectedFolderId = $state<string | null>(null);
  let query = $state("");

  // editor
  let editing = $state<{ phrase: Phrase | null; folderId: string } | null>(null);
  let showSettings = $state(false);

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
  }
  async function refreshPhrases() {
    phrases = await api.listPhrases(selectedFolderId, query.trim() || undefined);
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

  // reload phrases whenever the folder filter or search changes
  $effect(() => {
    void selectedFolderId;
    void query;
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
            📁 {f.name}
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
      <strong>Clipboard</strong>
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
    <div class="row between" style="margin-bottom:10px">
      <input bind:value={query} placeholder="Search phrases…" style="flex:1" />
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
    padding: 10px 12px;
    margin-bottom: 8px;
  }
  .phrase:hover {
    border-color: var(--accent);
  }
  .preview {
    margin-top: 4px;
    font-size: 13px;
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
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.4);
  }
</style>
