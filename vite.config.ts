import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
const { version } = JSON.parse(
  readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"),
);
const builtAt = new Date().toISOString();
let revision = "local";
try {
  revision = execFileSync("git", ["rev-parse", "--short", "HEAD"], {
    encoding: "utf8",
  }).trim();
  if (
    execFileSync("git", ["status", "--porcelain"], { encoding: "utf8" }).trim()
  )
    revision += "-modified";
} catch {
  /* Source archives can be built without Git metadata. */
}
const number =
  process.env.BC_BUILD_NUMBER ||
  `${builtAt.replace(/[-:TZ.]/g, "").slice(0, 14)}-${revision}`;

// Port fixed for Tauri (devUrl in tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  define: { __APP_BUILD__: JSON.stringify({ version, number, builtAt }) },
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
