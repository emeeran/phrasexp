<script lang="ts">
  let {
    draft,
    dirty,
    saving,
    onedit,
    onsave,
  }: {
    /** Current draft values for the action (null while the action lookup hasn't resolved). */
    draft: { name: string; prompt_template: string } | null;
    dirty: boolean;
    saving: boolean;
    onedit: (patch: Partial<{ name: string; prompt_template: string }>) => void;
    onsave: () => void;
  } = $props();
</script>

{#if draft}
  <div class="ai-action-edit">
    <input
      value={draft.name}
      oninput={(e) => onedit({ name: e.currentTarget.value })}
      aria-label="Action name"
      placeholder="Action name"
    />
    <textarea
      rows={3}
      value={draft.prompt_template}
      oninput={(e) => onedit({ prompt_template: e.currentTarget.value })}
      aria-label="Prompt template"
      placeholder="Rewrite professionally:&#10;&#123;clipboard&#125;"
    ></textarea>
    <div class="row" style="justify-content:space-between; align-items:center">
      <span class="caption" style="font-size:var(--fs-xs)">
        Shared action — every phrase and popup using it gets this template.
      </span>
      <button
        class="tiny primary"
        disabled={!dirty || saving}
        onclick={onsave}
      >{saving ? "Saving…" : "Save action"}</button>
    </div>
  </div>
{/if}

<style>
  .ai-action-edit {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-top: 1px dashed var(--border-soft);
    padding-top: var(--sp-2);
  }
  .ai-action-edit textarea {
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
  }
  .caption {
    font-size: var(--fs-sm);
    color: var(--text-dim);
    font-weight: 500;
  }
</style>
