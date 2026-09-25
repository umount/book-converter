# Native manga runtime

Cloud recognition and dialogue translation run through the shared provider transport.
Masks, inpainting and lettering run locally in the
[manga-inference](../crates/manga-inference/) child process. The desktop application
supplies resolved paths and image hashes through a versioned JSON protocol. The worker
has no provider credentials or project database connection.

## Packaged components

[prepare-manga-runtime.mjs](../scripts/prepare-manga-runtime.mjs) builds the worker with
ONNX support, fetches the pinned ONNX Runtime archive, checks its SHA-256 and writes
`src-tauri/manga-runtime/`. The output contains the executable, runtime libraries,
license notices and a hashed resource manifest checked by local capability and processing setup.

The runtime pins and target list are in [manga-runtime.json](../scripts/manga-runtime.json):
Linux x86-64, Windows x86-64 MSVC, and macOS x86-64/ARM64. Entries describe build inputs;
they do not substitute for testing installers on those platforms. The current runtime
pin is ONNX Runtime 1.22.0.

```bash
npm run manga:prepare
```

Both Tauri development and packaged builds run this automatically through their
build hooks. Downloaded model weights alone do not supply the worker or runtime DLLs. Downloads are cached under `.cache/manga-runtime` by
default; `MANGA_RUNTIME_CACHE` overrides that location. `TAURI_ENV_TARGET_TRIPLE`
selects the packaging target when provided. Model weights are not bundled by this step.

## Model catalog

The source of truth for exact revisions, sizes and hashes is
[models/catalog.rs](../src-tauri/src/models/catalog.rs):

- `comic-text-mask-resnet18`: `TareHimself/comic-text-mask`,
  `model.safetensors`, used for text-pixel masks.
- `lama-onnx-fp32`: `Carve/LaMa-ONNX`, `lama_fp32.onnx`, used for inpainting.

The model manager downloads these artifacts from pinned Hugging Face revisions and
verifies them before installation. Catalog entries currently carry an experimental
flag. License strings record the model repositories' declarations; bundled code/font
notices are in [third-party](../third-party/) and copied into the runtime pack.
Lettering uses the bundled font/shaping implementation rather than another model download.

## Execution and publication

The host verifies the runtime pack and resolves installed model paths. Each operation
gets a temporary workspace; the worker validates request version, input paths/hashes
and operation parameters. The host timeout is 300 seconds. Outputs are checked before
being registered as immutable project assets. The domain pipeline rechecks captured
revisions before committing result metadata and the successful job step together.

Automatic segmentation produces text-pixel masks. Explicit edited rectangles can request
full rectangular cleanup. Lettering checks glyph coverage and layout fit, and reports
text overflow instead of silently clipping a successful result. Native failures stay
inside the worker process and become structured application errors.

## Diagnostics and verification

- **Runtime missing/damaged:** logs identify the missing path or a manifest/target/size/hash mismatch. Prepare resources again during development, or check
  that the installed application includes its `manga-runtime` directory.
- **Model missing:** install the required catalog artifact in Settings.
- **Font coverage:** the current font cannot render required characters.
- **Text overflow:** adjust region geometry or translated text before rebuilding.
- **Worker timeout:** the local operation exceeded the configured host deadline.

Unit and worker tests do not download weights. Opt-in native tests require explicit
runtime/model paths and exercise actual CPU inference on synthetic pages. Commands
are in [Development](DEVELOPMENT.md). Review representative manga pages separately
for segmentation quality, preserved artwork and typography.

Enable **Settings → Diagnostics → Full diagnostic logging** and export the log ZIP
after reproducing a failure. See [Windows diagnostic builds](DEVELOPMENT.md#windows-diagnostic-installer-for-testers).
