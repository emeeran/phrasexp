<script lang="ts">
  // PopupAiResult — shows the AI rewrite over the captured selection and asks what to
  // do with it: Replace (over the selection), Insert (after it), Copy (to clipboard),
  // Retry, or Discard. When the response contains multiple options (a bare list, or
  // sections the parser can extract responses from), only the responses are shown —
  // the model's preamble and "Why it works" rationale are dropped — as rows with
  // inline Insert/Copy. Picking a row collapses the view to just that response, and
  // every decision acts on the picked text alone.
  // When the original selection is known (sent with ai-result), a Diff toggle renders
  // the rewrite as deletions/insertions — grammar edits show exactly what changed.
  // Presentational: keys are handled by the Popup orchestrator; decisions go back
  // through ondecide → api.aiDecide.
  import { charDiff } from "./diff";
  import { splitAiOptions, type AiOption } from "./aiOptions";
  let {
    result,
    original = "",
    busy,
    streaming = false,
    ondecide,
  }: {
    result: string;
    /** The captured selection, when known — enables the diff preview. */
    original?: string;
    busy: boolean;
    /** True while tokens are still arriving — only Cancel is offered. */
    streaming?: boolean;
    ondecide: (d: "replace" | "insert" | "copy" | "retry" | "discard", text?: string) => void;
  } = $props();

  /** A response that came back as ≥2 recognizable items (a bare list, or headed
   *  "Option N" sections) becomes rows; anything else stays one blob. */
  const options = $derived(splitAiOptions(result));
  /** Index into `options` once the user picks one; null = show everything. */
  let picked = $state<number | null>(null);
  $effect(() => {
    // A retry swaps in a fresh response — drop any stale selection.
    void result;
    picked = null;
  });

  // Digits 1–9 pick an option straight from the keyboard. Safe alongside the Popup
  // orchestrator's svelte:window handler: it ignores bare digits in result mode.
  function onkeydown(e: KeyboardEvent) {
    if (options.length < 2 || picked !== null || busy || streaming) return;
    const n = Number(e.key);
    if (n >= 1 && n <= Math.min(options.length, 9)) {
      e.preventDefault();
      picked = n - 1;
    }
  }

  /** The text the decisions act on: the picked option's response, or the full text. */
  const chosen = $derived(picked !== null ? options[picked].text : result);
  // Diff preview: char-level LCS is O(m·n), so only offer it for selection-sized
  // texts; a whole-document rewrite would allocate gigabytes for nothing.
  const canDiff = $derived(
    !!original.trim() &&
      original !== result &&
      original.length + result.length < 4000,
  );
  let showDiff = $state(false);
  const diff = $derived(showDiff && canDiff ? charDiff(original, chosen) : null);
  /** Replace/Insert/Copy wait for a pick — acting on the raw multi-option list is
   *  never what the user wants in the target document. */
  const needsPick = $derived(options.length > 1 && picked === null);
  const noPickHint = "Pick an option first";
  /** Instance export: lets the Popup orchestrator's Enter/I/C keys respect the
   *  pick-first rule the buttons already enforce, and stand down mid-stream. */
  export function canDecide(): boolean {
    return !needsPick && !streaming;
  }
  /** Copy honors a mouse-highlighted portion of the response; without one it takes
   *  the picked option (or the whole response). Instance export so the orchestrator's
   *  C key takes the same path as the button. */
  export function decideCopy() {
    const sel = window.getSelection()?.toString().trim();
    ondecide("copy", sel || chosen);
  }
</script>

<svelte:window onkeydown={onkeydown} />

<div class="col" style="gap:6px; flex:1; min-height:0">
  <div class="head" data-tauri-drag-region>
    <span class="caption" data-tauri-drag-region>
      {#if streaming}
        Generating…
      {:else if options.length > 1}
        {picked !== null ? `Option ${picked + 1} of ${options.length}` : `${options.length} options`}
      {:else}
        AI result
      {/if}
    </span>
    {#if canDiff && !streaming}
      <button
        class="back caption"
        class:on={showDiff}
        disabled={busy}
        onclick={() => (showDiff = !showDiff)}
        title="Show what changed against the original selection"
      >diff</button>
    {/if}
  </div>
  {#if options.length > 1 && picked === null && !streaming}
    <div class="opts">
      {#each options as o, i (i)}
        <div
          class="opt"
          role="button"
          tabindex="-1"
          onclick={() => (picked = i)}
          onkeydown={(e) => e.key === "Enter" && (picked = i)}
        >
          <span class="key">{i + 1}</span>
          <div class="optmain">
            {#if o.label}<span class="optlabel">{o.label}</span>{/if}
            <span class="opttext">{o.text}</span>
          </div>
          <!-- Inline decisions: act on this option without picking it first.
               (Svelte 5 runes: no event modifiers — stop the row's pick by hand.) -->
          <span class="optacts">
            <button
              class="mini"
              disabled={busy}
              title="Insert this option after the selection"
              onclick={(e) => {
                e.stopPropagation();
                ondecide("insert", o.text);
              }}>Insert</button
            >
            <button
              class="mini"
              disabled={busy}
              title="Copy this option to the clipboard"
              onclick={(e) => {
                e.stopPropagation();
                ondecide("copy", o.text);
              }}>Copy</button
            >
          </span>
        </div>
      {/each}
    </div>
  {:else}
    {#if options.length > 1 && !streaming}
      <button class="back caption" disabled={busy} onclick={() => (picked = null)}>
        ← all {options.length} options
      </button>
    {/if}
    {#if diff}
      <!-- aria-live: streaming deltas + picks are announced, not silent. -->
      <div class="result diff" role="status" aria-live="polite">
        {#if picked === null && options.length > 1}
          <span class="caption" style="display:block; padding-bottom:4px">Diffing the picked option once chosen — full list shown above.</span>
        {/if}
        <span class="old">{#each diff.left as c}{#if c.type === "del"}<del>{c.ch}</del>{:else}{c.ch}{/if}{/each}</span>
        <span class="new">{#each diff.right as c}{#if c.type === "ins"}<ins>{c.ch}</ins>{:else}{c.ch}{/if}{/each}</span>
      </div>
    {:else}
      <!-- aria-live: streaming deltas + picks are announced, not silent. -->
      <div class="result" role="status" aria-live="polite">{chosen}</div>
    {/if}
  {/if}
  <div class="choices">
    {#if streaming}
      <button class="choice danger" onclick={() => ondecide("discard")}>
        Cancel <span class="key">Esc</span>
      </button>
    {:else}
      <button
        class="choice primary"
        disabled={busy || needsPick}
        title={needsPick ? noPickHint : ""}
        onclick={() => ondecide("replace", chosen)}
      >
        Replace <span class="key">Enter</span>
      </button>
      <button
        class="choice"
        disabled={busy || needsPick}
        title={needsPick ? noPickHint : ""}
        onclick={() => ondecide("insert", chosen)}
      >
        Insert <span class="key">I</span>
      </button>
      <button
        class="choice"
        disabled={busy || needsPick}
        title={needsPick ? noPickHint : "Copy — or just highlight part of the response"}
        onclick={() => decideCopy()}
      >
        Copy <span class="key">C</span>
      </button>
      <button class="choice" disabled={busy} onclick={() => ondecide("retry")}>
        Retry <span class="key">R</span>
      </button>
      <button class="choice danger" disabled={busy} onclick={() => ondecide("discard")}>
        Discard <span class="key">Esc</span>
      </button>
    {/if}
  </div>
  {#if busy && !streaming}
    <div class="caption" style="padding:0 2px">Working…</div>
  {/if}
</div>

<style>
  .head {
    padding: 2px 2px 0;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .result {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    white-space: pre-wrap;
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    padding: 8px;
    font-size: var(--fs-base);
    line-height: 1.45;
  }
  .opts {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .opt {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    padding: 7px 8px;
    font-size: var(--fs-base);
    line-height: 1.4;
    color: var(--text);
    cursor: pointer;
  }
  .opt:hover,
  .opt:focus-visible {
    background: var(--accent-soft);
    border-color: var(--accent);
    outline: none;
  }
  .optmain {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .optlabel {
    font-weight: 500;
  }
  .opttext {
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-dim);
  }
  .optacts {
    flex: none;
    display: flex;
    gap: 4px;
    /* Align with the first text line, not the row top edge. */
    padding-top: 1px;
  }
  .mini {
    padding: 1px 7px;
    font-size: var(--fs-xs);
    background: none;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    color: var(--accent);
  }
  .mini:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .back {
    background: none;
    border: none;
    color: var(--text-faint);
    text-align: left;
    padding: 0 2px;
    cursor: pointer;
  }
  .back:hover:not(:disabled) {
    color: var(--accent);
  }
  .back.on {
    color: var(--accent);
    font-weight: 500;
  }
  .result.diff .old {
    display: block;
    margin-bottom: 6px;
    padding-bottom: 6px;
    border-bottom: 1px dashed var(--border-soft);
  }
  .result.diff del {
    color: var(--danger);
    text-decoration-thickness: 1.5px;
  }
  .result.diff ins {
    color: var(--ok);
    text-decoration: none;
    font-weight: 500;
  }
  .choices {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .choice {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 7px 6px;
    font-size: var(--fs-base);
  }
  .key {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    border: 1px solid var(--border);
    border-radius: var(--radius-xs);
    padding: 0 4px;
  }
</style>
