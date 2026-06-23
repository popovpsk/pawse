import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      "/api": "http://localhost:8770",
      "/ws": { target: "ws://localhost:8770", ws: true },
    },
  },
});
