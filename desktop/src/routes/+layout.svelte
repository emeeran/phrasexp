<script lang="ts">
  import "../app.css";
  import { applyTheme } from "$lib/theme";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => applyTheme());
</script>

{@render children()}
