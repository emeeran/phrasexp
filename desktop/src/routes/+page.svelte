<script lang="ts">
  // Main entry. The popup window shares this SPA entry — the label branch is the
  // FIRST statement; the popup renders Popup and imports no shell code.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Popup from "$lib/components/popup/Popup.svelte";
  import AppShell from "$lib/components/shell/AppShell.svelte";

  const isPopup = getCurrentWindow().label === "popup";
</script>

{#if isPopup}
  <Popup />
{:else}
  <AppShell />
{/if}
