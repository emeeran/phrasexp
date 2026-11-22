<script lang="ts">
  import type { AiAction, CaseMode, ContentType, ExecuteMode, Folder, Macro, Phrase, PhraseSummary, Segment } from "$lib/types";
  import { errToast } from "$lib/stores/toastError";
  import { bodyPreview, buildSavePayload } from "$lib/editor/phraseDraft";
  import TagInput from "$lib/components/TagInput.svelte";
  import { api, errMsg } from "$lib/invoke";
  import { fmtDateTime } from "$lib/dates";
  import { toast } from "$lib/stores/toast.svelte";
  import MacroPicker from "./MacroPicker.svelte";
  import MacroFields from "./MacroFields.svelte";
  import TriggerFields from "./TriggerFields.svelte";
  import VariationsPanel from "./VariationsPanel.svelte";

  let {
    phrase = null,
    folderId,
    initialText = "",
    folders = [],
    phrases = [],
    onsaved,
    oncancel,
  }: {
    phrase?: Phrase | null;
    folderId: string;
    initialText?: string;
    folders?: Folder[];
    phrases?: PhraseSummary[];
    onsaved: (p: Phrase) => void;
    oncancel: () => void;
  } = $props();

  /** Phrases available to link (exclude self to avoid trivial self-recursion). */
  const linkable = $derived(phrases.filter((p) => p.id !== phrase?.id));

  // svelte-ignore state_referenced_locally
  const isEdit = !!phrase;
  // Snapshot the prop once — the editor is a fresh snapshot, not a live view of `phrase`.
  // svelte-ignore state_referenced_locally
  const src = phrase;

  let name = $state(src?.name ?? "");
  let abbreviation = $state(src?.abbreviation ?? "");
  let hotkey = $state(src?.triggers.find((t) => t.type === "hotkey")?.key ?? "");
  let isRegex = $state(src?.triggers.find((t) => t.type === "autotext")?.regex ?? false);
  let execute = $state<ExecuteMode>(
    src?.triggers.find((t) => t.type === "autotext")?.execute ?? "immediate",
  );
  let appScope = $state(src?.app_scope ?? "");
  // Tags as the comma-joined string TagInput edits; split + normalized by the backend.
  let tagsText = $state(src?.tags?.join(", ") ?? "");
  let contentType = $state<ContentType>(src?.content_type ?? "plain");
  // Capture the mount-time folder like `src` — the {#key} block remounts the
  // editor per session, so a live binding isn't wanted here.
  // svelte-ignore state_referenced_locally
  let editFolderId = $state(src?.folder_id ?? folderId);
  let sensitive = $state(src?.sensitive ?? false);
  let caseMode = $state<CaseMode>(src?.case_mode ?? "adapt");
  // svelte-ignore state_referenced_locally
  let segments: Segment[] = $state(
    src?.body ? structuredClone($state.snapshot(src.body)) : [{ type: "text", text: initialText }],
  );

  let error = $state<string | null>(null);
  let saving = $state(false);
  /** path -> data URL, for previewing image segments in this editor session. */
  let previews: Record<string, string> = $state({});
  /** AI action presets — feed the picker and the aiaction segment's select. */
  let aiActions: AiAction[] = $state([]);
  $effect(() => {
    api
      .listAiActions()
      .then((a) => (aiActions = a))
      .catch((e) => (error = errMsg(e)));
  });

  // Inline editing of an action's prompt template right in the aiaction segment —
  // no trip to Settings → AI. Edits the SHARED action (every phrase using it is
  // affected, which is the point: templates live in one place).
  let aiDrafts = $state<Record<string, { name: string; prompt_template: string }>>({});
  let aiSavingId = $state<string | null>(null);

  function aiDraftOf(id: string): { name: string; prompt_template: string } | null {
    const a = aiActions.find((x) => x.id === id);
    if (!a) return null;
    return aiDrafts[id] ?? { name: a.name, prompt_template: a.prompt_template };
  }
  function aiDirty(id: string): boolean {
    const d = aiDrafts[id];
    const a = aiActions.find((x) => x.id === id);
    return !!d && !!a && (d.name !== a.name || d.prompt_template !== a.prompt_template);
  }
  function editAi(id: string, patch: Partial<{ name: string; prompt_template: string }>) {
    const cur = aiDraftOf(id);
    if (cur) aiDrafts = { ...aiDrafts, [id]: { ...cur, ...patch } };
  }
  async function saveAi(id: string) {
    const cur = aiDraftOf(id);
    const a = aiActions.find((x) => x.id === id);
    if (!cur || !a) return;
    aiSavingId = id;
    try {
      const saved = await api.saveAiAction({ ...a, name: cur.name, prompt_template: cur.prompt_template });
      aiActions = aiActions.map((x) => (x.id === saved.id ? saved : x));
      const { [id]: _drop, ...rest } = aiDrafts;
      aiDrafts = rest;
      toast(`Saved action “${saved.name}”`, "ok");
    } catch (e) {
      errToast(e);
    } finally {
      aiSavingId = null;
    }
  }

  // ---- Undo (Ctrl+Z) — structural changes only; text fields keep native undo ---- //
  let undoStack: Segment[][] = [];
  function pushUndo() {
    undoStack.push($state.snapshot(segments) as Segment[]);
  }
  function onEditorKeydown(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.key.toLowerCase() !== "z") return;
    const tag = (e.target as HTMLElement | null)?.tagName ?? "";
    if (/^(input|textarea|select)$/i.test(tag)) return;
    e.preventDefault();
    const prev = undoStack.pop();
    if (prev) segments = prev;
  }

  function addText() {
    pushUndo();
    segments.push({ type: "text", text: "" });
  }

  /** Sensible starting point for each macro type (dates are dd-mm-yyyy app-wide). */
  function defaultMacro(type: Macro["type"]): Macro {
    switch (type) {
      case "inputform": return { type, field: "name", kind: "text", options: [] };
      case "datemath": return { type, days_offset: 0, format: "%d-%m-%Y" };
      case "datetime": return { type, format: "%d-%m-%Y %H:%M" };
      case "random": return { type, min: 1, max: 100 };
      case "math": return { type, expr: "2+2" };
      case "nested": return { type, phrase_id: linkable[0]?.id ?? "" };
      case "env": return { type, var: "HOME" };
      case "pick": return { type, options: ["yes", "no"] };
      case "file": return { type, path: "" };
      case "ai": return { type, prompt: "Translate to French: " };
      case "aiaction": return { type, action_id: aiActions[0]?.id ?? "" };
      case "uuid": return { type };
      case "shell": return { type, command: "" };
      case "counter": return { type, name: "ticket" };
      case "delay": return { type, ms: 500 };
      case "cursor": return { type };
      case "cite": return { type, source: "", style: "apa" };
      default: return { type: "clipboard" };
    }
  }
  function addMacro(type: Macro["type"]) {
    pushUndo();
    segments.push({ type: "macro", macro: defaultMacro(type) });
  }
  function removeAt(i: number) {
    pushUndo();
    segments.splice(i, 1);
  }
  function move(i: number, dir: number) {
    const j = i + dir;
    if (j < 0 || j >= segments.length) return;
    pushUndo();
    const tmp = segments[i];
    segments[i] = segments[j];
    segments[j] = tmp;
  }

  /** Any image on the clipboard becomes an image segment (stored as a PNG on disk). */
  async function onPaste(e: ClipboardEvent) {
    const item = [...(e.clipboardData?.items ?? [])].find((i) => i.type.startsWith("image/"));
    const file = item?.getAsFile();
    if (!file) return;
    e.preventDefault();
    try {
      const dataUrl = await toPngDataUrl(file);
      const path = await api.saveImage(dataUrl);
      previews[path] = dataUrl;
      pushUndo();
      segments.push({ type: "macro", macro: { type: "image", path } });
    } catch (err) {
      error = errMsg(err);
    }
  }

  /** Normalize to PNG — the Rust side only ever decodes PNG. */
  async function toPngDataUrl(blob: Blob): Promise<string> {
    if (blob.type === "image/png") {
      return await new Promise((resolve, reject) => {
        const r = new FileReader();
        r.onload = () => resolve(r.result as string);
        r.onerror = () => reject(r.error);
        r.readAsDataURL(blob);
      });
    }
    const bmp = await createImageBitmap(blob);
    const canvas = document.createElement("canvas");
    canvas.width = bmp.width;
    canvas.height = bmp.height;
    canvas.getContext("2d")!.drawImage(bmp, 0, 0);
    return canvas.toDataURL("image/png");
  }

  // Preview images referenced by a saved body (fresh pastes set previews directly).
  $effect(() => {
    for (const s of segments) {
      if (s.type === "macro" && s.macro.type === "image" && !previews[s.macro.path]) {
        const path = s.macro.path;
        void api.readImage(path).then((url) => (previews[path] = url));
      }
    }
  });

  // ---- AI variations (PhraseExpress v18-style): reword the first text segment, then
  // swap it for a Pick macro so every expansion varies between original + rewrites ---- //
  const firstText = $derived(
    (segments.find((s) => s.type === "text") as { text: string } | undefined)?.text ?? "",
  );
  let varying = $state(false);
  let variations = $state<string[]>([]);
  let variationPicks = $state<Set<number>>(new Set());

  async function generateVariations() {
    const text = firstText.trim();
    if (!text || varying) return;
    varying = true;
    error = null;
    try {
      const vars = await api.aiVariations(text, 3);
      if (vars.length === 0) {
        error = "AI returned no variations — try again";
        return;
      }
      variations = vars;
      variationPicks = new Set(vars.map((_, i) => i)); // all pre-selected
    } catch (e) {
      error = errMsg(e);
    } finally {
      varying = false;
    }
  }

  function applyVariations() {
    const chosen = variations.filter((_, i) => variationPicks.has(i));
    if (chosen.length === 0) return;
    pushUndo();
    const idx = segments.findIndex((s) => s.type === "text");
    segments.splice(idx, 1, {
      type: "macro",
      macro: { type: "pick", options: [firstText, ...chosen] },
    });
    variations = [];
    variationPicks = new Set();
  }

  // 3.5 live preview: flat shape of the body as it's typed (text verbatim,
  // macros as {label} placeholders) — mirrors what the expander will produce
  // without resolving anything.
  const preview = $derived(bodyPreview($state.snapshot(segments) as Segment[]));

  // 3.5 abbreviation-conflict warning: saving an abbreviation another live phrase
  // already owns makes BOTH fire (multi-match chooser on every use). Confirm so
  // the overlap is a decision, not a surprise.
  async function confirmAbbrevConflict(abbrev: string, selfId?: string): Promise<boolean> {
    const trimmed = abbrev.trim();
    if (!trimmed || isRegex) return true;
    try {
      const all = await api.listPhrases(undefined, undefined, false, "name");
      const clash = all.find(
        (p) =>
          p.id !== selfId &&
          (p.abbreviation ?? "").trim().toLowerCase() === trimmed.toLowerCase(),
      );
      if (!clash) return true;
      return confirm(
        `The abbreviation "${trimmed}" is already used by "${clash.name}".\n` +
          `Both will trigger and a chooser pops up on every use.\n\nSave anyway?`,
      );
    } catch {
      return true; // conflict check is advisory — never block saving on it
    }
  }

  async function save() {
    error = null;
    if (!(await confirmAbbrevConflict(abbreviation, phrase?.id))) return;
    // Payload shape + validation live in $lib/editor/phraseDraft (pure, unit-tested).
    const body = $state.snapshot(segments) as Segment[];
    const built = buildSavePayload({
      name,
      abbreviation,
      hotkey,
      isRegex,
      execute,
      appScope,
      tagsText,
      contentType,
      caseMode,
      sensitive,
      segments: body,
      editFolderId,
      folderId,
      firstFolderId: folders[0]?.id,
    });
    if ("error" in built) return (error = built.error);
    saving = true;
    try {
      const saved: Phrase =
        isEdit && phrase
          ? await api.updatePhrase(phrase.id, built.payload)
          : await api.createPhrase(built.payload);
      onsaved(saved);
    } catch (e) {
      error = errMsg(e);
    } finally {
      saving = false;
    }
  }
</script>

<svelte:window onkeydown={onEditorKeydown} />

<div class="col" style="gap:14px" onpaste={onPaste}>
  <div class="row between">
    <h3 style="margin:0">{isEdit ? "Edit phrase" : "New phrase"}</h3>
    <div class="row" style="gap:6px">
      {#if phrase?.expanded_count}
        <span
          class="tag muted"
          title={phrase.last_used_at ? `last used ${fmtDateTime(phrase.last_used_at)}` : undefined}
        >used ×{phrase.expanded_count}</span>
      {/if}
    </div>
  </div>

  <div class="field">
    <label for="pe-name">Name</label>
    <input id="pe-name" bind:value={name} placeholder="Signature" />
  </div>

  <div class="row" style="gap:10px">
    <div class="field" style="flex:1">
      <label for="pe-folder">Folder</label>
      <select id="pe-folder" bind:value={editFolderId}>
        {#each folders as f (f.id)}
          <option value={f.id}>{f.name}</option>
        {/each}
      </select>
    </div>
    <div class="field" style="flex:1">
      <label for="pe-ct">Content type</label>
      <select
        id="pe-ct"
        value={contentType}
        onchange={(e) => (contentType = e.currentTarget.value as ContentType)}
      >
        <option value="plain">Plain text (typed)</option>
        <option value="markdown">Markdown (pasted as formatted HTML)</option>
      </select>
    </div>
  </div>

  <div class="field">
    <span class="label" id="pe-case-label">Case adaptation</span>
    <div class="seg" role="radiogroup" aria-labelledby="pe-case-label">
      <button type="button" class="seg-btn" class:on={caseMode === "adapt"} role="radio" aria-checked={caseMode === "adapt"}
        title="Follow the global setting (Settings → Features)"
        onclick={() => (caseMode = "adapt")}>Match typed</button>
      <button type="button" class="seg-btn" class:on={caseMode === "always"} role="radio" aria-checked={caseMode === "always"}
        title="Always adapt: sig → lowercase, Sig → Title Case, SIG → ALL CAPS"
        onclick={() => (caseMode = "always")}>Always</button>
      <button type="button" class="seg-btn" class:on={caseMode === "never"} role="radio" aria-checked={caseMode === "never"}
        title="Never change the case — output the body exactly as written"
        onclick={() => (caseMode = "never")}>Never</button>
    </div>
    <span class="faint" style="font-size:var(--fs-xs)">sig / Sig / SIG shapes the expansion — or override per phrase here.</span>
  </div>

  <TriggerFields bind:abbreviation bind:hotkey bind:isRegex bind:execute bind:appScope />

  <div class="field">
    <label for="pe-tags">Tags <span class="faint">(Enter or comma to add — find later with <code>#tag</code> in search)</span></label>
    <TagInput id="pe-tags" bind:value={tagsText} placeholder="e.g. sales, invoices" />
  </div>

  <label class="row" style="gap:8px; cursor:pointer">
    <input type="checkbox" bind:checked={sensitive} style="width:auto" />
    <span class="muted">Sensitive (masked in lists)</span>
  </label>

  <div class="col" style="gap:8px">
    <div class="row between">
      <strong>Body</strong>
      <div class="row" style="gap:6px">
        <button
          class="tiny"
          disabled={!firstText.trim() || varying}
          onclick={generateVariations}
          title={firstText.trim() ? "AI rewrites of the first text segment — expansions then pick one at random" : "Add a text segment first"}
        >
          {varying ? "…" : "✨ Variations"}
        </button>
        <MacroPicker
        disableNested={linkable.length === 0}
        {aiActions}
        onpick={(v) => {
          if (v === "text") {
            addText();
          } else if (v.startsWith("aiaction:")) {
            pushUndo();
            segments.push({
              type: "macro",
              macro: { type: "aiaction", action_id: v.slice("aiaction:".length) },
            });
          } else {
            addMacro(v as Macro["type"]);
          }
        }}
      />
      </div>
    </div>
    {#each segments as seg, i (i)}
      <div class="segment">
        <div class="row between">
          <span class="tag">{seg.type === "text" ? "text" : `macro · ${seg.macro.type}`}</span>
          <div class="row">
            <button class="tiny ghost" onclick={() => move(i, -1)}>↑</button>
            <button class="tiny ghost" onclick={() => move(i, 1)}>↓</button>
            <button class="tiny danger" onclick={() => removeAt(i)}>✕</button>
          </div>
        </div>
        <MacroFields
          {seg}
          {linkable}
          {previews}
          {aiActions}
          {aiDraftOf}
          {aiDirty}
          {editAi}
          {saveAi}
          {aiSavingId}
        />
      </div>
    {/each}

    <VariationsPanel bind:variations bind:variationPicks onapply={applyVariations} />
  </div>

  {#if preview.trim()}
    <div class="preview-box">
      <span class="preview-label">live preview</span>
      <span class="preview-text">{preview}</span>
    </div>
  {/if}

  {#if error}<div class="error">{error}</div>{/if}

  <div class="row" style="justify-content:flex-end">
    <button onclick={oncancel}>Cancel</button>
    <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
  </div>
</div>

<style>
  .preview-box {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    background: var(--surface-2);
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
  }
  .preview-label {
    font-size: var(--fs-xs);
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .preview-text {
    white-space: pre-wrap;
    word-break: break-word;
    font-size: var(--fs-base);
    color: var(--text-dim);
    max-height: 140px;
    overflow-y: auto;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .seg-btn {
    background: var(--surface-1);
    border: none;
    border-right: 1px solid var(--border-soft);
    padding: 5px 12px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    cursor: pointer;
  }
  .seg-btn:last-child {
    border-right: none;
  }
  .seg-btn.on {
    background: var(--accent);
    color: var(--accent-ink, #fff);
    font-weight: 500;
  }
  .seg-btn:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .segment {
    background: var(--bg-elev);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    padding: var(--sp-3);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    transition: border-color var(--t-fast), box-shadow var(--t-fast);
  }
  .segment:hover {
    border-color: var(--border);
    box-shadow: var(--shadow-sm);
  }
</style>
