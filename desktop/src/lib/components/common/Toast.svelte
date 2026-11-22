<script lang="ts">
  // aria-live toast region — renders the toast store; entry springs in, fade out.
  import { toasts, dismissToast } from "$lib/stores/toast.svelte";
  import { fly } from "svelte/transition";
  import Icon, { type IconName } from "./Icon.svelte";

  const kindIcon = (kind: string): IconName => (kind === "ok" ? "check" : "info");
</script>

{#if toasts.items.length > 0}
  <div class="toast-region" role="status" aria-live="polite">
    {#each toasts.items as t (t.id)}
      <div class={`toast ${t.kind}`} transition:fly={{ y: 8, duration: 160 }}>
        <Icon name={kindIcon(t.kind)} size={13} />
        <span class="msg">{t.message}</span>
        <button class="ghost tiny dismiss" onclick={() => dismissToast(t.id)} aria-label="Dismiss">
          <Icon name="close" size={12} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-region {
    position: fixed;
    bottom: 44px; /* clear of the status bar */
    right: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 100;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 240px;
    max-width: 380px;
    padding: 9px 11px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-md);
    font-size: var(--text-dense);
  }
  .toast.ok {
    border-left-color: var(--ok);
  }
  .toast.err {
    border-left-color: var(--danger);
  }
  .toast.warn {
    border-left-color: var(--danger);
  }
  .toast .msg {
    flex: 1;
  }
  .dismiss {
    padding: 2px 4px;
    line-height: 0;
  }
</style>
