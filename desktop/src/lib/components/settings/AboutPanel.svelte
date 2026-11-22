<script lang="ts">
  // About panel — version, stack, links.
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";

  let appVersion = $state("…");
  onMount(async () => {
    appVersion = await getVersion().catch(() => "unknown");
  });
</script>

<div class="about">
  <img class="about-logo" src="/logo-white.png" alt="PhraseXp" width="56" height="56" />
  <h2>PhraseXp</h2>
  <span class="faint">v{appVersion} · Tauri 2 + Rust + Svelte 5</span>
  <p>Lightweight, local-first text expander. Type an abbreviation to expand it, summon a search popup with a global hotkey, or use macros, Markdown, AI, and spellcheck to automate your typing.</p>
  <div class="divider"></div>
  <p class="about-credit">Developed by <strong>Dawnstar</strong></p>
  <a href="https://github.com/emeeran/phrasexp" target="_blank" rel="noopener">GitHub ↗</a>
</div>

<style>
  .about { display: flex; flex-direction: column; gap: 4px; align-items: center; text-align: center; padding: var(--sp-6) 0; }
  .about h2 { margin: 0; font-family: var(--font-display); font-size: var(--fs-2xl); letter-spacing: var(--tracking-display); }
  .about p { margin: 4px 0; font-size: var(--fs-base); color: var(--text-dim); line-height: 1.5; max-width: 46ch; }
  .about-credit { font-size: var(--fs-md); color: var(--text); }
  .about a { color: var(--accent); font-size: var(--fs-base); }
  .divider { height: 1px; background: var(--border-soft); width: 120px; margin: 8px 0; }
  .about-logo { align-self: center; }
</style>
