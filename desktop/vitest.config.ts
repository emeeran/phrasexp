import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// Standalone test config — deliberately NOT the app's vite.config.js (whose
// sveltekit() plugin needs the full .svelte-kit context and dev-server flags).
// The svelte plugin is here so .svelte component tests can be added later;
// current tests target pure TS modules only.
export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL("./src/lib", import.meta.url)),
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
