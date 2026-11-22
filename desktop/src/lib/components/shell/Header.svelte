<script lang="ts">
  // Header — brand pill, autotext/spellcheck status dots, theme toggle.
  // (Duplicate finder lives in Settings → Data.)
  import { statusStore } from "$lib/stores/status.svelte";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { getTheme, toggleTheme } from "$lib/theme";
  import Icon from "../common/Icon.svelte";

  let theme = $state(getTheme());

  const spellcheckOn = $derived(
    settingsStore.values?.spellcheck_enabled !== false &&
      settingsStore.values?.spellcheck_enabled !== undefined,
  );

  // Per-OS fix-it for the blocked-listener state (webview-only detection is enough
  // — the hint only needs to name the user's own platform).
  const platform = navigator.platform.toLowerCase();
  const blockedHint = platform.startsWith("mac")
    ? "Listener not receiving keys. Grant PhraseXp Accessibility and Input Monitoring in System Settings → Privacy & Security, then restart the app."
    : platform.startsWith("win")
      ? "Listener not receiving keys. If a security policy blocks low-level keyboard hooks, run PhraseXp as a normal user and restart it."
      : "Listener not receiving keys. Add yourself to the 'input' group and re-login (`sudo usermod -aG input $USER`). On Wayland, also check ydotoold is running.";

  const live = $derived(
    statusStore.autotext?.enabled &&
      statusStore.autotext?.running &&
      (statusStore.autotext?.accessibility_granted ?? true),
  );
  const enabled = $derived(!!statusStore.autotext?.enabled);
  const secureInput = $derived(statusStore.autotext?.secure_input_active === true);
</script>

<div class="header">
  <div class="brand">
    <img class="brand-logo" src="/logo-white.png" alt="" width="22" height="22" />
    <span class="name">PhraseXp</span>
  </div>

  <div class="dots">
    {#if statusStore.autotext}
      <span
        class="dotpill {live ? 'ok' : enabled ? 'warn' : 'off'}"
        title={
          (enabled && !live
            ? blockedHint
            : `autotext: ${enabled ? "enabled" : "disabled"}`) +
          (secureInput
            ? " · Secure Input active (password field) — PhraseXp can't see keys until it's dismissed"
            : "")
        }
      >
        <span class="dot"></span>
        {enabled ? (live ? "live" : "blocked") : "off"}
      </span>
    {/if}
    <span
      class="dotpill {spellcheckOn ? 'ok' : 'off'}"
      title={`spellcheck: ${spellcheckOn ? "on" : "off"}`}
    >
      <span class="dot"></span>
      spell
    </span>
  </div>

  <div class="actions">
    <button
      class="ghost icon-btn"
      aria-label={`Theme: ${theme} (click to cycle dark → light → sepia)`}
      title={`Theme: ${theme} (click to cycle dark → light → sepia)`}
      onclick={() => (theme = toggleTheme())}
    >
      {#if theme === "dark"}
        <Icon name="theme" size={15} />
      {:else if theme === "light"}
        <Icon name="sun" size={15} />
      {:else}
        <Icon name="book" size={15} />
      {/if}
    </button>
  </div>
</div>

<style>
  .header {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    padding: 9px 14px;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border-soft);
  }
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .brand .name {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: var(--fs-md);
    letter-spacing: var(--tracking-display);
  }
  .dots {
    display: flex;
    gap: var(--sp-3);
    flex: 1;
  }
  .dotpill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-dense);
    color: var(--text-dim);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .ok .dot {
    background: var(--ok);
    box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.15);
  }
  .warn .dot {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
  }
  .off .dot {
    background: var(--dot-off);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .icon-btn {
    width: 30px;
    height: 30px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
  }
</style>
