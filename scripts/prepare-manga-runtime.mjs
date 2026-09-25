// Build-time tool only. End users receive the worker/runtime, never Python or Cargo.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rename,
  rm,
  stat,
  writeFile,
  chmod,
} from "node:fs/promises";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pins = JSON.parse(
  await readFile(join(root, "scripts/manga-runtime.json"), "utf8"),
);
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(
  /^host: (.+)$/m,
)?.[1];
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const pin = pins.targets[target];
if (!pin) throw new Error(`Unsupported manga runtime target: ${target}`);
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const cache = resolve(
  process.env.MANGA_RUNTIME_CACHE || join(root, ".cache", "manga-runtime"),
);
await mkdir(cache, { recursive: true });
const archive = join(cache, pin.archive);
let bytes;
try {
  bytes = await readFile(archive);
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
if (!bytes || digest(bytes) !== pin.sha256) {
  const response = await fetch(
    `https://github.com/microsoft/onnxruntime/releases/download/v${pins.version}/${pin.archive}`,
  );
  if (!response.ok)
    throw new Error(`Runtime download: HTTP ${response.status}`);
  bytes = Buffer.from(await response.arrayBuffer());
  if (digest(bytes) !== pin.sha256)
    throw new Error("Runtime archive SHA-256 mismatch");
  await writeFile(archive, bytes);
}
const temporary = await mkdtemp(join(tmpdir(), "book-converter-runtime-"));
const destination = join(root, "src-tauri", "manga-runtime");
try {
  // Only an archive with the pinned digest reaches the extractor.
  if (pin.archive.endsWith(".zip") && process.platform === "win32") {
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Expand-Archive -LiteralPath $env:BC_ARCHIVE -DestinationPath $env:BC_EXTRACT",
      ],
      {
        env: { ...process.env, BC_ARCHIVE: archive, BC_EXTRACT: temporary },
        stdio: "inherit",
      },
    );
  } else
    execFileSync("tar", ["-xf", archive, "-C", temporary], {
      stdio: "inherit",
    });
  const cargoTarget = join(root, "crates", "manga-inference", "target");
  execFileSync(
    "cargo",
    [
      "build",
      "--locked",
      "--release",
      "--manifest-path",
      join(root, "crates", "manga-inference", "Cargo.toml"),
      "--features",
      "onnx",
      "--target",
      target,
    ],
    {
      stdio: "inherit",
      env: { ...process.env, CARGO_TARGET_DIR: cargoTarget },
    },
  );
  const staged = join(temporary, "staged");
  await mkdir(staged);
  const executable = target.includes("windows")
    ? "manga-inference.exe"
    : "manga-inference";
  await copyFile(
    join(cargoTarget, target, "release", executable),
    join(staged, executable),
  );
  const extracted = join(temporary, pin.archive.replace(/\.(tgz|zip)$/, ""));
  for (const file of pin.libraries)
    await copyFile(join(extracted, "lib", file), join(staged, file));
  for (const file of ["LICENSE", "ThirdPartyNotices.txt"])
    await copyFile(join(extracted, file), join(staged, `ONNX-${file}`));
  for (const file of ["koharu-LICENSE-MIT", "DejaVu-LICENSE"])
    await copyFile(join(root, "third-party", file), join(staged, file));
  await copyFile(
    join(root, "LICENSE.md"),
    join(staged, "Book-Converter-LICENSE.md"),
  );
  if (process.platform !== "win32")
    await chmod(join(staged, executable), 0o755);
  if (target === host)
    execFileSync(
      join(staged, executable),
      ["verify-runtime", join(staged, pin.libraries[0])],
      { stdio: "inherit" },
    );
  const files = {};
  for (const file of [executable, ...pin.libraries])
    files[file] = {
      sha256: digest(await readFile(join(staged, file))),
      bytes: (await stat(join(staged, file))).size,
    };
  await writeFile(
    join(staged, "manifest.json"),
    JSON.stringify(
      {
        version: 1,
        target,
        onnxVersion: pins.version,
        archiveSha256: pin.sha256,
        files,
      },
      null,
      2,
    ) + "\n",
  );
  await writeFile(join(staged, ".gitkeep"), "");
  // The old generated pack is replaced only after extraction/build/load checks succeed.
  const previous = destination + ".previous";
  await rm(previous, { recursive: true, force: true });
  try {
    await rename(destination, previous);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  try {
    // Copy through a sibling directory so final rename stays on the same filesystem.
    const next = destination + ".next";
    await rm(next, { recursive: true, force: true });
    await mkdir(next, { recursive: true });
    const { readdir } = await import("node:fs/promises");
    for (const file of await readdir(staged))
      await copyFile(join(staged, file), join(next, file));
    if (process.platform !== "win32")
      await chmod(join(next, executable), 0o755);
    await rename(next, destination);
  } catch (error) {
    await rename(previous, destination).catch(() => {});
    throw error;
  }
  await rm(previous, { recursive: true, force: true });
  console.log(
    `Manga runtime prepared for ${target}. Model weights are not bundled.`,
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
