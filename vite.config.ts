import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port fixed for Tauri (devUrl in tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Don't watch the Rust side — Tauri rebuilds it.
      ignored: ["**/src-tauri/**"],
    },
  },
});
