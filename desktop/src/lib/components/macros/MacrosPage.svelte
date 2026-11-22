<script lang="ts">
  // Macros page — gallery of the 11 macro kinds with a live-preview playground.
  // Mirrors the Rust resolver (src-tauri/src/domain/expand.rs).
  import Icon from "../common/Icon.svelte";

  interface MacroRef {
    kind: string;
    blurb: string;
    example: string;
  }
  const MACRO_KINDS: MacroRef[] = [
    { kind: "datetime", blurb: "Current date/time, chrono/strftime format.", example: "%Y-%m-%d %H:%M" },
    { kind: "datemath", blurb: "A date offset by N days.", example: "+7 days, format %Y-%m-%d" },
    { kind: "math", blurb: "Evaluate an arithmetic expression.", example: "round(10/3) → 3 · 100*1.2" },
    { kind: "random", blurb: "Random integer in [min, max].", example: "min=1, max=6" },
    { kind: "pick", blurb: "One random choice from a list.", example: "yes, no, maybe" },
    { kind: "inputform", blurb: "Prompt for a value when you expand (text / select / checkbox).", example: "field=ship_to, kind=select" },
    { kind: "clipboard", blurb: "Insert the current clipboard contents.", example: "{clipboard}" },
    { kind: "nested", blurb: "Expand another phrase inline (by reference).", example: "phrase → ;sig" },
    { kind: "env", blurb: "An environment variable.", example: "HOME, USER" },
    { kind: "file", blurb: "Insert a file's contents by path.", example: "/etc/hostname" },
    { kind: "ai", blurb: "An LLM transforms text from a prompt (configure on the AI page).", example: "{clipboard} splices the clipboard into the prompt" },
  ];
</script>

<div class="page">
  <div class="head">
    <h2>Macros</h2>
    <p class="muted">
      Dynamic segments resolved when a phrase expands. Add them from a phrase editor's
      "+ segment" menu and mix them freely with plain text — each macro's output is
      spliced inline.
    </p>
  </div>

  <div class="gallery scroll">
    {#each MACRO_KINDS as m (m.kind)}
      <div class="card">
        <div class="row between">
          <span class="tag mono">{m.kind}</span>
          <Icon name="bolt" size={14} />
        </div>
        <p class="blurb">{m.blurb}</p>
        <code class="example">{m.example}</code>
      </div>
    {/each}
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .head {
    padding: 14px 18px 10px;
  }
  .head h2 {
    margin: 0 0 4px;
    font-family: var(--font-display);
    font-size: var(--text-display);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
  }
  .head p {
    margin: 0;
    max-width: 70ch;
    font-size: var(--fs-base);
  }
  .gallery {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 10px;
    padding: 4px 18px 18px;
    align-content: start;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
    transition: border-color var(--t-fast), transform var(--t-fast), box-shadow var(--t-fast);
  }
  .card:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
    box-shadow: var(--shadow-sm);
  }
  .blurb {
    margin: 0;
    font-size: var(--fs-base);
    color: var(--text-dim);
    flex: 1;
  }
  .example {
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--accent);
    background: var(--accent-soft);
    padding: 3px 7px;
    border-radius: var(--radius-xs);
    width: fit-content;
  }
</style>
