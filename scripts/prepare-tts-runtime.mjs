import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const testing = args.includes("--test");
const python = testing ? join(root, ".cache/tts-venv", process.platform === "win32" ? "Scripts/python.exe" : "bin/python")
  : process.env.BOOK_TTS_PYTHON || (process.platform === "win32" ? "python" : "python3");
const result = spawnSync(python, testing ? ["-m", "unittest", "discover", "-s", "tests/tts", "-v"]
  : [join(root, "scripts/prepare-tts-runtime.py"), ...args], { cwd: root, stdio: "inherit" });
if (result.error) console.error(result.error.message);
process.exitCode = result.status ?? 1;
