import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

// Compile the pure TypeScript boundary with the installed compiler, then use Node's test runner.
const output = mkdtempSync(join(tmpdir(), "book-converter-tests-"));
try {
  const build = spawnSync(process.execPath, ["node_modules/typescript/bin/tsc",
    "--target", "ES2020", "--module", "commonjs", "--strict", "--skipLibCheck",
    "--outDir", output, "src/shared/api/transport.ts"], { stdio: "inherit" });
  if (build.error) throw build.error;
  if (build.status !== 0) process.exitCode = build.status ?? 1;
  else {
    writeFileSync(join(output, "package.json"), '{"type":"commonjs"}');
    const tests = spawnSync(process.execPath, ["--test", "tests/frontend/transport.test.mjs"], {
      stdio: "inherit", env: { ...process.env, FRONTEND_TEST_OUTPUT: output },
    });
    if (tests.error) throw tests.error;
    process.exitCode = tests.status ?? 1;
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
