<script lang="ts">
  import "../app.css";
  import { applyTheme, setTheme, type Theme } from "$lib/theme";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => {
    applyTheme();
    // A theme toggle in any window broadcasts `theme-changed`; apply it here so every
    // window (incl. the long-lived popup) stays matched. Sync onMount + cleanup kept sync
    // to avoid the async-onMount return type.
    let unlisten: (() => void) | undefined;
    listen<Theme>("theme-changed", (e) => setTheme(e.payload)).then((u) => (unlisten = u));
    return () => unlisten?.();
  });
</script>

{@render children()}
