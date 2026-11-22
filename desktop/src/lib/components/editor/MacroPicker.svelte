<script lang="ts">
  // MacroPicker — visual "+ segment" menu grouped by category, replacing the old
  // 18-option <select>. Keyboard accessible (buttons in a popover); Esc closes.
  import { fly } from "svelte/transition";
  import Icon, { type IconName } from "../common/Icon.svelte";
  import type { AiAction } from "$lib/types";

  type Entry = { value: string; label: string; hint?: string };

  let { onpick, aiActions = [], disableNested = false }: {
    onpick: (v: string) => void;
    /** AI action presets from the ai_actions table — one picker entry each. */
    aiActions?: AiAction[];
    disableNested?: boolean;
  } = $props();

  // AI actions come from the DB; "aiaction:<id>" values are unwrapped by the caller.
  const groups = $derived.by<Array<{ title: string; icon: IconName; entries: Entry[] }>>(() => [
    {
      title: "Basics",
      icon: "plus",
      entries: [{ value: "text", label: "Text", hint: "literal text" }],
    },
    {
      title: "Date & numbers",
      icon: "bolt",
      entries: [
        { value: "datetime", label: "Date/time", hint: "strftime format" },
        { value: "datemath", label: "Date math", hint: "± N days" },
        { value: "random", label: "Random number", hint: "min–max" },
        { value: "math", label: "Math", hint: "arithmetic expr" },
        { value: "pick", label: "Pick from list", hint: "random choice" },
        { value: "counter", label: "Counter", hint: "auto-increments each expand" },
        { value: "delay", label: "Delay", hint: "pause N ms mid-expansion" },
        { value: "cursor", label: "Cursor position", hint: "caret ends here (fill-in blank)" },
      ],
    },
    {
      title: "Context",
      icon: "copy",
      entries: [
        { value: "clipboard", label: "Clipboard", hint: "current clipboard text" },
        { value: "env", label: "Environment variable", hint: "HOME, USER…" },
        { value: "file", label: "File contents", hint: "by path" },
        { value: "nested", label: "Nested phrase", hint: "expand another phrase" },
        { value: "uuid", label: "UUID", hint: "fresh random v4 id" },
        { value: "shell", label: "Shell command", hint: "insert command output" },
        { value: "cite", label: "Citation", hint: "APA/MLA/Chicago from DOI or title" },
      ],
    },
    {
      title: "Interactive",
      icon: "info",
      entries: [
        { value: "inputform", label: "Input form", hint: "prompt on expand" },
      ],
    },
    {
      title: "AI",
      icon: "ai",
      entries: [
        { value: "ai", label: "Custom prompt", hint: "LLM transforms text" },
        ...aiActions.map((a): Entry => ({
          value: `aiaction:${a.id}`,
          label: a.name,
          hint: a.builtin ? "action · editable in Settings → AI" : "custom action",
        })),
      ],
    },
  ]);

  let open = $state(false);
  let rootEl: HTMLElement | undefined = $state();

  // Close on outside click.
  $effect(() => {
    if (!open) return;
    const onDoc = (ev: MouseEvent) => {
      if (rootEl && !rootEl.contains(ev.target as Node)) open = false;
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  });

  function pick(v: string) {
    onpick(v);
    open = false;
  }
</script>

<div class="picker" bind:this={rootEl}>
  <button class="primary" onclick={() => (open = !open)} aria-expanded={open}>
    + Segment
  </button>
  {#if open}
    <div
      class="menu"
      transition:fly={{ y: -6, duration: 140 }}
      role="menu"
      tabindex="-1"
      aria-label="Add segment"
      onkeydown={(e) => e.key === "Escape" && (open = false)}
    >
      {#each groups as g (g.title)}
        <div class="group">
          <div class="group-title">
            <Icon name={g.icon} size={11} />
            {g.title}
          </div>
          {#each g.entries as en (en.value)}
            <button
              class="entry"
              role="menuitem"
              disabled={en.value === "nested" && disableNested}
              title={en.value === "nested" && disableNested ? "No other phrases to link yet" : en.hint}
              onclick={() => pick(en.value)}
            >
              <span class="label">{en.label}</span>
              {#if en.hint}<span class="hint">{en.hint}</span>{/if}
            </button>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 30;
    min-width: 240px;
    max-height: 320px;
    overflow-y: auto;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-lg);
    padding: 4px;
  }
  .group + .group {
    border-top: 1px solid var(--border-soft);
    margin-top: 3px;
    padding-top: 3px;
  }
  .group-title {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 8px 3px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  .entry {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: var(--radius-xs);
    padding: 6px 8px;
    font-size: var(--fs-base);
    color: var(--text);
  }
  .entry:hover:not(:disabled),
  .entry:focus-visible {
    background: var(--accent-soft);
    color: var(--accent);
    outline: none;
  }
  .entry:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .hint {
    font-size: var(--fs-xs);
    color: var(--text-faint);
    white-space: nowrap;
  }
</style>
