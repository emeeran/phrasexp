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
  import Icon from "../common/Icon.svelte";
  let {
    result,
    original = "",
    busy,
    streaming = false,
    chips = [],
    activeChip = null,
    onchip,
    ondecide,
  }: {
    result: string;
    /** The captured selection, when known — enables the diff preview. */
    original?: string;
    busy: boolean;
    /** True while tokens are still arriving — only Cancel is offered. */
    streaming?: boolean;
    /** Quick re-run styles (first entry is "As-Is"): one tap re-runs the same
     *  selection through another builtin while the popup stays up. */
    chips?: { id: string; label: string }[];
    /** The action id that produced the CURRENT result — the highlighted chip. */
    activeChip?: string | null;
    onchip?: (actionId: string) => void;
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
    <span class="headtitle" data-tauri-drag-region>
      <Icon name="ai" size={13} />
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
        class="toggle"
        class:on={showDiff}
        disabled={busy}
        onclick={() => (showDiff = !showDiff)}
        title="Show what changed against the original selection"
      >
        <Icon name="eye" size={12} /> diff
      </button>
    {/if}
    <button
      class="close"
      disabled={busy && !streaming}
      aria-label="Discard"
      title="Discard — keep your original text (Esc)"
      onclick={() => ondecide("discard")}
    >
      <Icon name="close" size={14} />
    </button>
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
      <div class="result" class:streaming role="status" aria-live="polite">{chosen}</div>
    {/if}
  {/if}
  {#if !streaming && chips.length > 0}
    <div class="chips">
      {#each chips as c (c.id)}
        <button
          class="chip"
          class:on={c.id === activeChip}
          disabled={busy}
          title="Re-run the selection in this style — the result swaps in place"
          onclick={() => onchip?.(c.id)}
        >
          {c.label}
        </button>
      {/each}
    </div>
  {/if}
  <div class="choices">
    {#if streaming}
      <button class="choice cancel" onclick={() => ondecide("discard")}>
        <Icon name="close" size={12} /> Cancel <span class="key">Esc</span>
      </button>
    {:else}
      <button
        class="choice primary"
        disabled={busy || needsPick}
        title={needsPick ? noPickHint : "Paste over the selected text"}
        onclick={() => ondecide("replace", chosen)}
      >
        Replace <span class="key onprimary">Enter</span>
      </button>
      <button
        class="choice"
        disabled={busy || needsPick}
        title={needsPick ? noPickHint : "Paste after the selection, keeping the original"}
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
      <button class="choice" disabled={busy} title="Run the same style again" onclick={() => ondecide("retry")}>
        Retry <span class="key">R</span>
      </button>
    {/if}
  </div>
  {#if busy && !streaming}
    <div class="working" role="status">Working<span class="dots"><i></i><i></i><i></i></span></div>
  {/if}
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 2px 4px;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .headtitle {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: 1;
    min-width: 0;
    font-size: var(--fs-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--accent);
    white-space: nowrap;
    overflow: hidden;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px;
    font-size: var(--fs-xs);
    background: none;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    color: var(--text-faint);
    transition: color 150ms, border-color 150ms, background-color 150ms;
  }
  .toggle:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
  }
  .toggle.on {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    flex: none;
    background: none;
    border: none;
    border-radius: var(--radius-xs);
    color: var(--text-faint);
    transition: color 150ms, background-color 150ms;
  }
  .close:hover:not(:disabled) {
    color: var(--danger);
    background: var(--danger-soft);
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
  /* Streaming affordance: a soft pulsing edge light instead of a static box. */
  .result.streaming {
    border-color: var(--accent);
    animation: breathe 1.4s ease-in-out infinite;
  }
  @keyframes breathe {
    0%, 100% { border-color: var(--accent); }
    50% { border-color: var(--border-soft); }
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
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    flex: none;
  }
  .chip {
    padding: 2px 10px;
    font-size: var(--fs-base);
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    color: var(--text-dim);
    transition: color 150ms, border-color 150ms, background-color 150ms;
  }
  .chip:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .chip.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 500;
  }
  .chip:disabled {
    opacity: 0.5;
    cursor: wait;
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
    transition: filter 150ms, background-color 150ms, border-color 150ms;
  }
  .choice.cancel {
    flex: 1;
    background: var(--danger-soft);
    border-color: transparent;
    color: var(--danger);
  }
  .choice.cancel:hover:not(:disabled) {
    border-color: var(--danger);
  }
  .key {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    border: 1px solid var(--border);
    border-radius: var(--radius-xs);
    padding: 0 4px;
  }
  .key.onprimary {
    color: var(--accent-contrast);
    border-color: rgba(255, 255, 255, 0.4);
  }
  .working {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 2px;
    font-size: var(--fs-xs);
    color: var(--text-faint);
  }
  .working .dots {
    display: inline-flex;
    gap: 3px;
    margin-left: 4px;
  }
  .working .dots i {
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--text-faint);
    animation: hop 1.2s ease-in-out infinite;
  }
  .working .dots i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .working .dots i:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes hop {
    0%, 60%, 100% { transform: translateY(0); opacity: 0.5; }
    30% { transform: translateY(-3px); opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .result.streaming,
    .working .dots i {
      animation: none;
    }
  }
</style>
