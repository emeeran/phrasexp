<script lang="ts">
  // HotkeyInput — press-to-record chord field. Captures the next key chord and
  // emits a PhraseXp hotkey string ("Control+Alt+G") that the Rust parsers
  // understand (letters, digits, F1–F12, space/return/tab/backspace/escape/
  // comma/backslash). Esc cancels; blank = hotkey disabled.
  let { value = "", onchange }: { value?: string; onchange: (v: string) => void } = $props();

  let recording = $state(false);
  let hint = $state<string | null>(null);

  const NAMED: Record<string, string> = {
    " ": "space",
    Enter: "Return",
    Escape: "Escape",
    Tab: "Tab",
    Backspace: "Backspace",
    ",": "comma",
    "\\": "backslash",
  };

  function tokenFor(e: KeyboardEvent): string | null {
    if (/^[a-z0-9]$/i.test(e.key)) return e.key.toLowerCase();
    if (/^F([1-9]|1[0-2])$/.test(e.key)) return e.key.toLowerCase();
    return NAMED[e.key] ?? null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = false;
      hint = null;
      return;
    }
    if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return; // modifier held
    const mods: string[] = [];
    if (e.ctrlKey) mods.push("Control");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    const token = tokenFor(e);
    if (!token) {
      hint = "Unsupported key — letters, digits, F1–F12, space, comma";
      return;
    }
    if (mods.length === 0) {
      hint = "Needs at least one modifier (Ctrl / Alt / Shift / Super)";
      return;
    }
    hint = null;
    recording = false;
    onchange([...mods, token].join("+"));
  }

  function start() {
    recording = true;
    hint = null;
  }
</script>

<svelte:window onkeydowncapture={onkeydown} />

<div class="hotkey-input">
  <input value={value} readonly placeholder="none (disabled)" spellcheck="false" />
  {#if recording}
    <button class="record on" type="button" onclick={() => (recording = false)}>Press keys…</button>
  {:else}
    <button class="record" type="button" onclick={start}>Record</button>
  {/if}
  {#if value}
    <button class="clear" type="button" title="Disable the hotkey" onclick={() => onchange("")}>✕</button>
  {/if}
</div>
{#if hint}<span class="hint">{hint}</span>{/if}

<style>
  .hotkey-input {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  input {
    flex: 0 1 170px;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }
  button {
    white-space: nowrap;
  }
  .record.on {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: var(--accent);
  }
  .clear {
    padding: 2px 8px;
  }
  .hint {
    display: block;
    margin-top: 2px;
    font-size: var(--fs-xs);
    color: var(--warning, #b8860b);
  }
</style>
