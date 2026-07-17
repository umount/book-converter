import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Порт фиксирован под Tauri (devUrl в tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Не следим за Rust-стороной — её пересобирает Tauri.
      ignored: ["**/src-tauri/**"],
    },
  },
});
