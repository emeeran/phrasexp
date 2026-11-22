<script lang="ts">
  // Icon-rail sidebar — the five destinations. Active tile is a bold
  // accent fill; keyboard accessible with aria-current.
  import { app, type Route } from "$lib/stores/app.svelte";
  import Icon, { type IconName } from "../common/Icon.svelte";

  const items: Array<{ route: Route; icon: IconName; label: string }> = [
    { route: "library", icon: "library", label: "Library" },
    { route: "clipboard", icon: "clipboard", label: "Clipboard" },
    { route: "macros", icon: "macros", label: "Macros" },
    { route: "ai", icon: "ai", label: "AI" },
    { route: "settings", icon: "settings", label: "Settings" },
  ];
</script>

<nav class="rail" aria-label="Primary">
  {#each items as it (it.route)}
    <button
      class="tile"
      class:on={app.route === it.route}
      aria-current={app.route === it.route ? "page" : undefined}
      aria-label={it.label}
      title={it.label}
      onclick={() => app.routeTo(it.route)}
    >
      <Icon name={it.icon} size={19} stroke={1.8} />
    </button>
  {/each}
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 8px;
    background: var(--surface-1);
    border-right: 1px solid var(--border-soft);
    align-items: center;
  }
  .tile {
    width: 38px;
    height: 38px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    transition:
      background var(--t-fast),
      color var(--t-fast),
      border-color var(--t-fast);
  }
  .tile:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .tile.on {
    background: var(--accent-solid);
    border-color: var(--accent-solid);
    color: var(--accent-ink);
  }
  .tile:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
</style>
