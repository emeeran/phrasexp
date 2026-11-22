<script lang="ts">
  // Accessible modal: focus trap, Esc close, backdrop click, labelled dialog.
  // `open` is a prop; closing is reported through `onclose` (parent owns state).
  import { fade, scale } from "svelte/transition";

  let {
    open = false,
    title,
    onclose,
    children,
  }: {
    open?: boolean;
    title: string;
    onclose?: () => void;
    children: import("svelte").Snippet;
  } = $props();

  let dialogEl: HTMLElement | undefined = $state();
  let lastFocused: HTMLElement | null = null;

  function close() {
    onclose?.();
  }

  $effect(() => {
    if (!open || !dialogEl) return;
    lastFocused = document.activeElement as HTMLElement | null;
    dialogEl.querySelector<HTMLElement>("button, input, select, textarea, [tabindex]")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      } else if (e.key === "Tab" && dialogEl) {
        const focusables = dialogEl.querySelectorAll<HTMLElement>(
          "button, input, select, textarea, a[href], [tabindex]:not([tabindex='-1'])",
        );
        if (focusables.length === 0) return;
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      lastFocused?.focus();
    };
  });
</script>

{#if open}
  <div
    class="overlay"
    role="presentation"
    transition:fade={{ duration: 120 }}
    onclick={(e) => e.target === e.currentTarget && close()}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      transition:scale={{ duration: 160, start: 0.97, opacity: 0.6 }}
      bind:this={dialogEl}
      style="width: min(560px, 90vw)"
    >
      <div class="row between modal-head">
        <h3>{title}</h3>
        <button class="ghost tiny" onclick={close}>✕ close</button>
      </div>
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .modal-head {
    margin-bottom: 10px;
  }
  h3 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--fs-md);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
  }
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(3px);
    -webkit-backdrop-filter: blur(3px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 20px 24px;
    max-height: 85vh;
    overflow-y: auto;
    box-shadow: var(--shadow-lg);
  }
</style>
