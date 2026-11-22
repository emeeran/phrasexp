<script lang="ts">
  // Loading skeleton — shimmer rows for lists and panes.
  let { rows = 4, height = 46 }: { rows?: number; height?: number } = $props();
</script>

<div class="skeleton" aria-hidden="true">
  {#each Array.from({ length: rows }) as _, i (i)}
    <div class="sk-row" style={`height:${height - 10}px`}></div>
  {/each}
</div>

<style>
  .skeleton {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 4px 0;
  }
  .sk-row {
    border-radius: var(--radius-sm);
    background: linear-gradient(
      100deg,
      var(--surface-1) 40%,
      var(--surface-2) 50%,
      var(--surface-1) 60%
    );
    background-size: 200% 100%;
    animation: shimmer 1.2s linear infinite;
  }
  @keyframes shimmer {
    from { background-position: 120% 0; }
    to { background-position: -80% 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .sk-row { animation: none; }
  }
</style>
