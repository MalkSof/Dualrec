import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri espera un puerto fijo y no necesita que Vite limpie la consola.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
