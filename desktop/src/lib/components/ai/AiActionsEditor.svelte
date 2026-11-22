<script lang="ts">
  // AI actions editor — CRUD over the ai_actions table. Builtins (seeded on first
  // run) are editable but not deletable; custom actions are fully managed here.
  import { api } from "$lib/invoke";
  import { toast } from "$lib/stores/toast.svelte";
  import { errToast } from "$lib/stores/toastError";
  import type { AiAction } from "$lib/types";

  let actions = $state<AiAction[]>([]);
  /** id -> working copy being edited (only rows the user touched). */
  let drafts = $state<Record<string, AiAction>>({});
  let savingId = $state<string | null>(null);
  let loading = $state(true);

  async function refresh() {
    loading = true;
    try {
      actions = await api.listAiActions();
      drafts = {};
    } catch (e) {
      errToast(e);
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    void refresh();
  });

  function draftOf(a: AiAction): AiAction {
    return drafts[a.id] ?? { ...a };
  }
  function edit(a: AiAction, patch: Partial<AiAction>) {
    drafts = { ...drafts, [a.id]: { ...draftOf(a), ...patch } };
  }
  function dirty(a: AiAction): boolean {
    const d = drafts[a.id];
    return !!d && (d.name !== a.name || d.prompt_template !== a.prompt_template);
  }

  async function save(a: AiAction) {
    savingId = a.id;
    try {
      const saved = await api.saveAiAction(draftOf(a));
      actions = actions.map((x) => (x.id === saved.id ? saved : x));
      const { [a.id]: _drop, ...rest } = drafts;
      drafts = rest;
      toast(`Saved action “${saved.name}”`, "ok");
    } catch (e) {
      errToast(e);
    } finally {
      savingId = null;
    }
  }

  async function remove(a: AiAction) {
    try {
      await api.deleteAiAction(a.id);
      actions = actions.filter((x) => x.id !== a.id);
      toast(`Deleted action “${a.name}”`, "ok");
    } catch (e) {
      errToast(e);
    }
  }

  async function create() {
    // Ids for custom actions are namespaced so they can never collide with a builtin.
    const id = `custom-${Date.now().toString(36)}`;
    const action: AiAction = {
      id,
      name: "New action",
      prompt_template: "Rewrite in a friendly tone:\n{clipboard}",
      sort_order: actions.length,
      builtin: false,
    };
    try {
      const saved = await api.saveAiAction(action);
      actions = [...actions, saved];
      toast("Action created", "ok");
    } catch (e) {
      errToast(e);
    }
  }
</script>

<div class="col" style="gap:10px">
  <div class="row between">
    <div>
      <strong>Actions</strong>
      <p class="faint" style="margin:2px 0 0; font-size:var(--fs-xs)">
        Prompt presets selectable in the phrase editor and in the selection popup.
        {"{clipboard}"} / {"{selection}"} insert the captured text; any other {"{token}"}
        (e.g. {"{language}"}, {"{length}"}) prompts once for a value when the action runs.
      </p>
    </div>
    <button class="tiny" onclick={create}>+ New action</button>
  </div>

  {#if loading}
    <span class="faint">Loading…</span>
  {:else}
    {#each actions as a (a.id)}
      {@const d = draftOf(a)}
      <div class="action-row">
        <div class="row" style="gap:8px; align-items:center">
          <span class="tag" class:custom={!a.builtin}>{a.builtin ? "builtin" : "custom"}</span>
          <input
            class="name-input"
            value={d.name}
            oninput={(e) => edit(a, { name: e.currentTarget.value })}
            aria-label="Action name"
          />
        </div>
        <textarea
          rows={2}
          value={d.prompt_template}
          oninput={(e) => edit(a, { prompt_template: e.currentTarget.value })}
          aria-label="Prompt template"
        ></textarea>
        <div class="row" style="justify-content:flex-end; gap:6px">
          {#if !a.builtin}
            <button class="tiny danger" onclick={() => remove(a)}>Delete</button>
          {/if}
          <button
            class="tiny primary"
            disabled={!dirty(a) || savingId === a.id}
            onclick={() => save(a)}
          >{savingId === a.id ? "Saving…" : "Save"}</button>
        </div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .action-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg-elev);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    padding: var(--sp-3);
  }
  .name-input {
    flex: 1;
    font-weight: 500;
  }
  .tag.custom {
    border-color: var(--accent);
    color: var(--accent);
  }
</style>
