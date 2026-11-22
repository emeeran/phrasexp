<script lang="ts">
  // Features panel — the three modules (expander / spellchecker / clipboard), each
  // independently switchable, plus app-general settings. Ported from the legacy tab.
  import { onMount } from "svelte";
  import { api } from "$lib/invoke";
  import TagInput from "$lib/components/TagInput.svelte";
  import type { Settings } from "$lib/types";

  let { s }: { s: Settings } = $props();

  // Whether the OS keychain is usable for at-rest encryption.
  let cryptoAvail = $state(true);
  onMount(async () => {
    cryptoAvail = (await api.cryptoStatus().catch(() => ({ available: true }))).available;
  });
</script>

<div class="col">
  <section class="card">
    <div class="card-h">Text Expander</div>
    <label class="toggle">
      <span class="toggle-text">Enabled <span class="faint">— expand abbreviations silently as you type</span></span>
      <span class="switch"><input type="checkbox" bind:checked={s.autotext_enabled} /><span class="slider"></span></span>
    </label>
    <div class="grid" class:dim={!s.autotext_enabled}>
      <label class="toggle">
        <span class="toggle-text">Case adaptation <span class="faint">— SIG → ALL CAPS, Sig → Title</span></span>
        <span class="switch"><input type="checkbox" bind:checked={s.case_adapt_enabled} /><span class="slider"></span></span>
      </label>
      <div class="field">
        <label for="hk">Popup hotkey <span class="faint">— search &amp; insert from anywhere</span></label>
        <input id="hk" bind:value={s.popup_hotkey} placeholder="Control+Alt+Space" />
      </div>
      <div class="field">
        <label for="atx-exclude">Never expand in <span class="faint">(window-title substrings; Enter to add)</span></label>
        <TagInput id="atx-exclude" bind:value={s.autotext_exclude_apps} placeholder="e.g. Bitwarden" />
      </div>
    </div>
  </section>

  <section class="card">
    <div class="card-h">Spellchecker</div>
    <label class="toggle">
      <span class="toggle-text">Enabled <span class="faint">— correct typos as you type; independent of the expander</span></span>
      <span class="switch"><input type="checkbox" bind:checked={s.spellcheck_enabled} /><span class="slider"></span></span>
    </label>
    <div class="grid" class:dim={!s.spellcheck_enabled}>
      <div class="field">
        <label for="sc-hk">Toggle hotkey</label>
        <input id="sc-hk" bind:value={s.spellcheck_hotkey} placeholder="Control+Shift+Space" />
      </div>
      <div class="field">
        <span class="faint">Mode, delivery, and app exclusions live in the Spellcheck tab.</span>
      </div>
      <label class="toggle">
        <span class="toggle-text">
          Review sentence (AI) <span class="faint">— after a sentence ends, offer a grammar fix in a popup. <strong>Sends typed text to your AI provider</strong> — needs AI enabled; spellcheck app exclusions apply. Off by default.</span>
        </span>
        <span class="switch"><input type="checkbox" bind:checked={s.review_sentence} /><span class="slider"></span></span>
      </label>
    </div>
  </section>

  <section class="card">
    <div class="card-h">Clipboard Capture</div>
    <label class="toggle">
      <span class="toggle-text">Enabled <span class="faint">— keep a history of everything you copy</span></span>
      <span class="switch"><input type="checkbox" bind:checked={s.clipboard_enabled} /><span class="slider"></span></span>
    </label>
    {#if s.clipboard_enabled}
      <div class="grid">
        <div class="field">
          <label for="clip-max">Max entry length <span class="faint">(chars; 0 = unlimited)</span></label>
          <input id="clip-max" type="number" min="0" bind:value={s.clipboard_max_length} placeholder="10000" />
        </div>
        <div class="field">
          <label for="clip-entries">Max entries <span class="faint">(count; 0 = unlimited)</span></label>
          <input id="clip-entries" type="number" min="0" bind:value={s.clipboard_max_entries} placeholder="100" />
        </div>
      </div>
    {/if}
  </section>

  <section class="card">
    <div class="card-h">General</div>
    <div class="grid">
      <label class="toggle">
        <span class="toggle-text">Launch on system login</span>
        <span class="switch"><input type="checkbox" bind:checked={s.autostart} /><span class="slider"></span></span>
      </label>
      <label class="toggle">
        <span class="toggle-text">Start minimized <span class="faint">— launch hidden to the tray; click the tray icon to show</span></span>
        <span class="switch"><input type="checkbox" bind:checked={s.start_minimized} /><span class="slider"></span></span>
      </label>
    </div>
    <label class="toggle">
      <span class="toggle-text">
        Encrypt sensitive data at rest
        <span class="faint">— sensitive phrases, the AI key, and clipboard history are AES-encrypted in the DB; key kept in the OS keychain. Re-encrypts/decrypts your library on save.</span>
      </span>
      <span class="switch"><input type="checkbox" bind:checked={s.encrypt_at_rest} /><span class="slider"></span></span>
    </label>
    {#if s.encrypt_at_rest && !cryptoAvail}
      <div class="banner warn">
        OS keychain unavailable (common on headless Linux). Encryption is off until a keychain
        (e.g. gnome-keyring) is present — data stays plaintext. No data has been lost.
      </div>
    {/if}
  </section>
</div>

<style>
  .col {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-1);
    border: 1px solid var(--border-soft);
    border-radius: var(--radius-sm);
  }
  .card-h {
    font-family: var(--font-display);
    font-weight: 600;
    font-size: var(--fs-md);
  }
  /* Settings compaction: pairs of rows flow two-up on wide panels, stack on narrow. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    align-items: start;
    column-gap: var(--sp-5);
    row-gap: var(--sp-1);
  }
  /* A module that's switched off: its sub-settings stay visible but sleep. */
  .dim {
    opacity: 0.55;
  }
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    cursor: pointer;
    padding: 1px 0;
  }
  .toggle-text { flex: 1; }
  .toggle-text .faint { display: block; font-size: var(--fs-xs); font-weight: 400; }
  .banner {
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-sm);
    font-size: var(--fs-sm);
    line-height: 1.4;
  }
  .banner.warn {
    background: color-mix(in srgb, #f0a020 14%, transparent);
    border: 1px solid color-mix(in srgb, #f0a020 40%, transparent);
  }
</style>
