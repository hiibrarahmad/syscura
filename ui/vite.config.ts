import { defineConfig } from "vite";
import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and fails if it is not available.
export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
