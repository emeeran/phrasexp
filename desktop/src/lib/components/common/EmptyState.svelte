<script lang="ts">
  // Empty state — display headline + hint + optional action. Token-gated display type.
  import Icon, { type IconName } from "./Icon.svelte";
  let {
    icon = "info" as IconName,
    title,
    hint,
    actionLabel,
    onaction,
  }: {
    icon?: IconName;
    title: string;
    hint?: string;
    actionLabel?: string;
    onaction?: () => void;
  } = $props();
</script>

<div class="empty">
  <div class="glyph"><Icon name={icon} size={26} stroke={1.5} /></div>
  <h3 class="title">{title}</h3>
  {#if hint}<p class="hint">{hint}</p>{/if}
  {#if actionLabel && onaction}
    <button class="primary" onclick={onaction}>{actionLabel}</button>
  {/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    padding: var(--sp-10) var(--sp-6);
    text-align: center;
    color: var(--text-dim);
  }
  .glyph {
    color: var(--text-faint);
    line-height: 0;
  }
  .title {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-display);
    line-height: var(--lh-display);
    letter-spacing: var(--tracking-display);
    color: var(--text);
  }
  .hint {
    margin: 0;
    max-width: 42ch;
    color: var(--text-dim);
    font-size: var(--fs-base);
  }
</style>
