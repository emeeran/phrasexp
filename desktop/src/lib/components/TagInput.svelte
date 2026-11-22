<script lang="ts">
  // A dynamic "tag" input: entries show as removable chips; type a value and press Enter
  // (or comma) to add it, × (or Backspace on empty) to remove. `value` is the comma-joined
  // string the settings layer stores, kept in sync via $bindable.
  let { value = $bindable(""), placeholder = "", id = "" } = $props();

  let draft = $state("");

  const items = $derived(
    (value ?? "")
      .split(",")
      .map((s) => s.trim())
      .filter((x) => x.length > 0),
  );

  function commit(next: string[]) {
    value = next.join(", ");
  }

  function add() {
    const v = draft.trim();
    draft = "";
    if (!v) return;
    if (!items.includes(v)) commit([...items, v]);
  }

  function remove(item: string) {
    commit(items.filter((x) => x !== item));
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      add();
    } else if (e.key === "Backspace" && draft === "" && items.length) {
      e.preventDefault();
      commit(items.slice(0, -1));
    }
  }
</script>

<div class="tags">
  {#each items as it (it)}
    <span class="chip">
      {it}
      <button type="button" aria-label={`Remove ${it}`} onclick={() => remove(it)}>×</button>
    </span>
  {/each}
  <input
    type="text"
    class="tag-field"
    {id}
    bind:value={draft}
    {placeholder}
    onkeydown={onkeydown}
    onchange={add}
  />
</div>

<style>
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding: 7px 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    min-height: 40px;
    cursor: text;
    transition: border-color var(--t-fast), box-shadow var(--t-fast);
  }
  .tags:focus-within {
    border-color: var(--accent);
    box-shadow: var(--shadow-focus);
  }
  /* Reset the global `input` styling inside the chip box. */
  .tag-field {
    flex: 1 1 auto;
    width: auto;
    min-width: 140px;
    background: transparent;
    border: none;
    padding: 2px 0;
    box-shadow: none;
    outline: none;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 4px 2px 9px;
    background: var(--accent-soft);
    color: var(--accent);
    border-radius: 999px;
    font-size: 12px;
    line-height: 1.4;
  }
  .chip button {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    padding: 0 2px 0 6px;
    margin-left: 2px;
    line-height: 1;
    font-size: 15px;
    border-radius: var(--radius-pill);
    min-width: 18px;
    min-height: 18px;
  }
  .chip button:hover {
    color: var(--danger);
  }
</style>
