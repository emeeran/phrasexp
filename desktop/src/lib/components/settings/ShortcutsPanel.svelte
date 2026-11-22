<script lang="ts">
  // Shortcuts panel — reference table (reads live settings for hotkeys).
  import { settingsStore } from "$lib/stores/settings.svelte";

  const shortcuts = $derived([
    { key: settingsStore.values?.popup_hotkey ?? "Control+Alt+Space", desc: "Open the search popup (searchable phrase picker)" },
    { key: settingsStore.values?.spellcheck_hotkey ?? "Control+Shift+Space", desc: "Toggle spellcheck on/off (set in Features)" },
    { key: settingsStore.values?.ai_selection_hotkey || "—", desc: "AI-rewrite the selected text (set in AI; needs AI enabled)" },
    { key: "Control+Alt+R", desc: "Repeat the last expansion into the focused app (re-emit the same output)" },
    { key: "← Left arrow", desc: "Undo last spellcheck correction (immediately after correction)" },
    { key: "↑ ↓ Enter", desc: "Navigate popup lists (inside the popup window)" },
    { key: "Esc", desc: "Dismiss the popup" },
    { key: "Enter / Tab", desc: "Accept popup selection; Enter confirms an input form" },
    { key: "Ctrl+Z", desc: "Undo a body change in the phrase editor (segments added, removed, or moved)" },
    { key: "Any abbreviation", desc: "Type it in any app → auto-expands (when autotext is on)" },
    { key: "Per-phrase hotkey", desc: "Set in the phrase editor → directly injects that phrase" },
    { key: "Type word + Space", desc: "Spellcheck auto-corrects typos (when enabled)" },
  ]);
</script>

<table class="shortcut-table">
  <thead>
    <tr><th>Shortcut</th><th>Action</th></tr>
  </thead>
  <tbody>
    {#each shortcuts as sc (sc.key)}
      <tr>
        <td><code>{sc.key}</code></td>
        <td>{sc.desc}</td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  .shortcut-table { width: 100%; border-collapse: collapse; font-size: var(--fs-base); }
  .shortcut-table th { text-align: left; color: var(--text-dim); padding: 4px 8px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); }
  .shortcut-table td { padding: 5px 8px; border-bottom: 1px solid var(--border-soft); }
  .shortcut-table code { background: var(--surface-2); padding: 2px 6px; border-radius: var(--radius-xs); font-family: var(--font-mono); font-size: var(--fs-sm); }
</style>
