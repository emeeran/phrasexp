<script lang="ts">
  // PopupAiArg — parameter prompt for AI actions whose template carries interactive
  // placeholders (e.g. `{language}` for Translate; `{length}` + `{format}` for
  // Summarize): one row per token, preset chips for one-tap answers, then run.
  // Presentational: Enter/Esc are handled by the Popup orchestrator.
  import type { ArgPrompt } from "$lib/aiArgs";
  import { argPromptFor } from "$lib/aiArgs";

  let {
    actionName,
    tokens,
    busy,
    params = $bindable({}),
    onrun,
    onback,
  }: {
    actionName: string;
    /** The template's interactive tokens, in order — one input each. */
    tokens: string[];
    busy: boolean;
    params?: Record<string, string>;
    onrun: (params: Record<string, string>) => void;
    onback: () => void;
  } = $props();

  const prompts = $derived(tokens.map((t) => ({ token: t, ...argPromptFor(t) })));
  const canRun = $derived(tokens.every((t) => (params[t] ?? "").trim() !== ""));
  // Focus the first token's input once mounted (its id is deterministic).
  $effect(() => {
    if (tokens.length > 0) document.getElementById(`aiarg-${tokens[0]}`)?.focus();
  });

  function onInputKeydown(e: KeyboardEvent, i: number) {
    if (e.key === "Enter") {
      e.preventDefault();
      if (canRun) onrun(params);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onback();
    } else if (e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
      // Move to the next token's input (Tab falls through natively on the last row).
      const next = prompts[i + 1];
      if (next && e.key === "ArrowDown") {
        e.preventDefault();
        document.getElementById(`aiarg-${next.token}`)?.focus();
      }
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (i > 0) document.getElementById(`aiarg-${prompts[i - 1].token}`)?.focus();
    }
  }
</script>

<div class="col" style="gap:4px; flex:1; min-height:0">
  <div class="head row between" data-tauri-drag-region>
    <span class="caption" data-tauri-drag-region>
      {actionName} — {prompts.map((p) => p.label).join(" · ")}
    </span>
    <button class="back" disabled={busy} onclick={onback}>← actions</button>
  </div>
  {#each prompts as p, i (p.token)}
    <div class="argrow">
      <label class="caption" for="aiarg-{p.token}">{p.label}</label>
      <input
        id="aiarg-{p.token}"
        bind:value={params[p.token]}
        class="lang"
        placeholder={p.placeholder}
        spellcheck="false"
        disabled={busy}
        onkeydown={(e) => onInputKeydown(e, i)}
      />
      {#if p.chips}
        <div class="chips">
          {#each p.chips as c (c)}
            <button
              class="chip"
              class:on={params[p.token] === c}
              disabled={busy}
              onclick={() => (params[p.token] = c)}
            >{c}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
  <div class="choices">
    <button class="choice primary" disabled={busy || !canRun} onclick={() => onrun(params)}>
      Run <span class="key">Enter</span>
    </button>
    <button class="choice danger" disabled={busy} onclick={onback}>
      Back <span class="key">Esc</span>
    </button>
  </div>
  {#if busy}
    <div class="caption" style="padding:4px 8px">Working…</div>
  {/if}
</div>

<style>
  .head {
    padding: 2px 8px 4px;
    border-bottom: 1px solid var(--border-soft);
    cursor: grab;
    user-select: none;
  }
  .head:active {
    cursor: grabbing;
  }
  .argrow {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 2px 8px;
  }
  .lang {
    width: 100%;
    padding: 7px 8px;
    font-size: var(--fs-base);
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-xs);
    color: var(--text);
  }
  .lang:focus {
    outline: none;
    border-color: var(--accent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    padding: 3px 9px;
    font-size: var(--fs-xs);
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border-soft);
    color: var(--text-dim);
    cursor: pointer;
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
  .back {
    background: none;
    border: none;
    color: var(--text-faint);
    font-size: var(--fs-xs);
    cursor: pointer;
    padding: 0;
  }
  .back:hover:not(:disabled) {
    color: var(--accent);
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
