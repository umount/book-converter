# Manga tooling assessment and resource requirements

> Reviewed 2026-09-23. Documentation review, not a completed runtime benchmark.
> Companion to [REFACTORING.md](REFACTORING.md) and [MANGA.md](MANGA.md).
> Windows, macOS and Linux are required platforms. Local models must be lightweight.

## Recommended baseline

Use an automatic pipeline: cloud recognition and translation, automatic region/mask
generation, lightweight local model-based cleanup, and deterministic local lettering.
No local LLM/VLM, Python environment, CUDA installation or dedicated GPU is required.
The cleanup model is downloaded on demand and is required before its processing stage
can run. Import and viewing remain available before setup, but are not a translation
fallback. There is no model-free or manual manga processing mode.

Preflight checks all required stage capabilities before starting a page/range/volume.
If a required model or API configuration is missing, show the exact setup action and
block processing; do not silently substitute rectangle fills or manual editing.

Split the pipeline into explicit ports:

1. `RegionDetector`: proposes text regions, orientation and reading order.
2. `TextRecognizer`: reads source text from identified regions; preserves their IDs.
3. `DialogueTranslator`: translates source text with glossary and page context.
4. `TextMaskGenerator`: proposes pixel masks independently of OCR text.
5. `Inpainter`: cleans masked pixels; never modifies immutable originals.
6. `LetteringRenderer`: lays out translated text inside editable placement areas.

One provider may implement multiple ports, but results, provenance and retries stay
separate. A vision model's rectangle is a suggestion, not a safe erasure mask.
Automatic detection and mask generation must pass acceptance before processing is
considered ready. Review and retry are not substitutes for missing automation.

## Tool assessment

### Cloud recognition: DeepSeek Flash is a valid candidate

Official documentation currently names `deepseek-flash` as image-capable and shows
Chat Completions messages containing image/text blocks. The older experimental
vision alias is retired. This supports using the existing provider transport after
adding multimodal content; it does not establish manga accuracy or precise geometry.
[Official vision guide](https://api-docs.deepseek.com/guides/vision/).

Use a whole-page overview for context and crops for small dialogue. Map coordinates
back to original pixels, validate region IDs and handle refusals/truncation. Check
current account access, payload limits and supported options during P09; do not
hardcode the old plan's unverified 48 MiB assumption. Record model ID and prompt
version for reproducibility. Do not silently change the book translation profile.

### Manga OCR: benchmark candidate, not the default dependency

The upstream recognizer targets printed Japanese manga, including vertical text and
furigana. It returns text, requires Python/PyTorch, and documents a roughly 400 MB
model download. That excludes runtime dependencies and says nothing about peak RAM.
It can hallucinate text on empty crops. It is not a detector or mask generator.
[Upstream Manga OCR](https://github.com/kha-white/manga-ocr).

Keep it as a Japanese crop-recognition benchmark or future optional adapter. Do not
bundle its Python stack in the baseline or assume an ONNX conversion is available
and equivalent. It is not a general solution for every source language.

### Detection and segmentation: a separate missing component

Comic Text Detector supplies text boxes, lines and segmentation, making it a useful
candidate for evaluating geometry and mask quality. Its repository is GPL-3.0;
record code and weight provenance separately before choosing distribution terms.
[Upstream detector](https://github.com/dmMaze/comic-text-detector).

Do not select it for shipping solely because it provides an ONNX artifact. First
measure download size, CPU memory/latency and manga-mask quality on all supported
architectures. Until automatic detection and segmentation pass, this stage remains
incomplete. Do not use a manual region or mask editor to satisfy its acceptance.

### Cleanup: a lightweight model is required; LaMa is a candidate

LaMa is image-and-mask inpainting; the reference implementation is a model pipeline,
not a turnkey Rust desktop component. A converted model needs independent numerical,
operator and image-quality validation.
[Upstream LaMa](https://github.com/advimman/lama).

`ort` primarily wraps ONNX Runtime. It does not make the runtime pure Rust, and
execution providers and native library packaging vary by platform.
[ort](https://github.com/pykeio/ort),
[execution-provider documentation](https://github.com/pykeio/ort/blob/main/docs/content/perf/execution-providers.mdx).

Evaluate CPU inference on bounded crops with surrounding context, one session at a
time. Composite only the approved mask back into the full-resolution original;
nonmasked decoded pixels must stay unchanged. Test crop seams and texture continuity.
Quantization and smaller input sizes are experiments, not assumed quality-preserving
optimizations. Reject any artifact that fails the resource budget.

Choose a lightweight inpainting model that passes the gates below. LaMa is not frozen
as the choice. If no candidate passes, record a release blocker rather than adding a
manual/model-free fallback or silently increasing hardware requirements. Failed or
low-quality pages remain `needs_review` and support automatic retry; they must not be
reported as successfully translated.

### Lettering: cosmic-text plus application layout logic

cosmic-text supplies shaping, layout, fallback and rasterization and documents full
support on Linux, macOS and Windows. It is a stronger starting point than choosing a
glyph rasterizer alone.
[Upstream cosmic-text](https://github.com/pop-os/cosmic-text).

The application still implements bubble fitting, outline/compositing, rotation,
font selection and visible overflow. Use the same Rust renderer for preview and
export. Bundle a small licensed font set covering supported target scripts, and
make additional script packs optional. Record font IDs/versions so system-font
variation does not change exported layout. Vertical target lettering and decorative
SFX require separate acceptance tests; do not infer them from generic Unicode support.

## Proposed lightweight acceptance budgets

These are initial engineering gates, not measured capabilities. P00 records baseline
hardware and P09/P10 report actual measurements. Do not silently relax the gates to
accommodate a preferred model.

- Baseline hardware: an 8 GiB RAM machine, CPU-only, no dedicated GPU.
- No model downloads on startup or ordinary project import. Show size before install.
- Required local model pack: at most 500 MiB downloaded and 1 GiB installed, including
  its added native runtime/dependencies. Sum all required artifacts for a capability.
- At most one resident inference session by default; release it after idle timeout
  or explicit disable. Avoid separate copies for simultaneous projects.
- Processing peak: target at most 2 GiB total process-tree RSS, including the UI,
  image buffers and any worker; report idle and peak separately.
- CPU processing target: p95 at most 30 seconds per ordinary page for local stages
  on the recorded baseline fixture set; report cold load separately. Network latency
  is measured separately and is not covered by this local target.
- Keep UI responsive; cancellation acknowledged within one second. A noninterruptible
  native inference runs in an isolatable worker with a bounded shutdown strategy.
- Bound image dimensions, crop size, decoder allocations, thumbnails and queued work.
  Large spreads and unusually large images must not cause uncontrolled allocation.

If a model fails these budgets, it cannot become the default local capability.
Record the failed experiment and evaluate another lightweight candidate. Additional heavy
profiles are outside this plan unless explicitly requested later.

## Cross-platform implementation gates

Required release targets: Windows x86_64, Linux x86_64, macOS arm64 and macOS x86_64.
Select and document minimum OS versions in P00 against the actual Tauri/runtime
support matrix. Other architectures may be added later; OS coverage alone does not
prove every architecture works.

- CPU execution is the mandatory baseline. Core ML, DirectML or CUDA are optional
  accelerators only after correctness/fallback checks; no accelerator is required.
- Bundle the correct native runtime per target. Test installed release builds, not
  only developer machines. Users should not have to install Python, Homebrew or a
  system ONNX Runtime to use the default workflow.
- Model registry records version, hash, byte sizes, licenses, input contract, runtime
  compatibility and supported OS/architecture. Downloads support progress, cancel,
  partial-file cleanup and atomic publication. Missing/corrupt packs disable only
  the corresponding capability, never book translation or project opening.
- Use platform app-data/cache paths and argument arrays for subprocesses. Test spaces,
  Cyrillic/CJK names, case sensitivity, Windows locks and atomic replacement behavior.
- RAR handling must work on all release targets through a reviewed packaged adapter
  or expose an extracted-folder fallback. Do not assume a Linux `unrar` executable
  exists. ZIP/CBZ and folder import remain available without external tools.
- Test font coverage, clipboard, native file dialogs, asset URLs and high-DPI canvas
  coordinates on all platforms. Test macOS signed/notarized packages and Windows
  packaged native-library discovery as part of release verification.
- CI builds/tests every target; native smoke tests exercise import, reopen, one-page
  automatic processing/render/export and required model loading. A Linux pass cannot close these gates.

## Evidence required before freezing the stack

Use 10–20 representative pages: vertical/horizontal Japanese, furigana, dense dialogue,
plain bubbles, text over artwork, blank art, SFX, color and a spread. Include synthetic
redistributable fixtures; do not commit copyrighted samples without permission.

Record detector recall and geometry overlap, OCR character error rate against checked
transcripts, missing/duplicated region IDs, reading order, translation omissions,
mask damage outside intended letters, overflow, visual cleanup quality, peak RSS,
download/install size, cold/warm CPU latency and cloud request cost. Check lossless
intermediate pixels outside masks exactly; evaluate lossy final exports separately.
P09 defines quality thresholds before comparing candidates, based on those fixtures.

Save exact model/weight hashes, runtime/provider versions, CPU/OS/architecture,
commands and failures in REFACTORING_STATUS.md. Prefer the simplest candidate that
passes all gates. Documentation support alone is not evidence of acceptable quality,
low resource use or a working packaged build.

## On-demand Hugging Face downloads (implemented 2026-09-24)

Model weights are not bundled. Settings exposes the application-owned catalog and
explicit download/pause/resume/remove actions. The cache is global under
`<app-data>/models/<model-id>-<sha256>/`, independent of projects and fixed language
choices. Closing Settings leaves downloads running; an app restart leaves a resumable
partial file. Import/open never starts a download. Public downloads use their own HTTP
client and never receive the book provider's API key. Private/gated repositories and
arbitrary repository URLs are not supported by this first catalog implementation.

The first **experimental download candidate**, not an accepted processing default:

- Repository: `Carve/LaMa-ONNX`; file: `lama_fp32.onnx`.
- Commit: `a3ee2fca54baebec351b8fa7786154ffa7555aa6`.
- Exact bytes: `208044816` (198.4 MiB); repository-declared license: Apache-2.0.
- SHA-256: `1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6`.
- Provenance: [pinned artifact](https://huggingface.co/Carve/LaMa-ONNX/blob/a3ee2fca54baebec351b8fa7786154ffa7555aa6/lama_fp32.onnx)
  and the Hugging Face model API at that same commit (`blobs=true`), checked 2026-09-24.
- Runtime/OS acceptance, model license/provenance review and image-quality benchmarks
  remain open. Downloaded/verified is deliberately distinct from processing readiness.
  Recognition/translation still use the planned cloud path; no local LLM stack is
  silently added and no downloaded Python code is executed.

Downloads always use the pinned commit, not `main`. HTTP ranges are validated against
both the local offset and exact total length. A server that ignores Range restarts the
file from zero. Truncated downloads retain received bytes; checksum failures discard
the corrupt partial file. Complete files are hashed before publication and checked
again when a fresh manager reads the installed cache. The eventual inference adapter
must verify artifacts at its own load boundary as well. A downloaded file does not
activate any manga processing capability.

The downloader permits one active write, streams to `.part`, restricts redirects to
HTTPS Hugging Face download domains, bounds bytes to the catalog size, and publishes
only verified files. Symlink cache entries are rejected. Cancellation interrupts HTTP
waits and retains the partial file; deleting a model's files is blocked while its writer
is active. No archive extraction or automatic model execution occurs.

Validation used tiny local HTTP fixtures, not a paid provider or a production weight
download. Tests cover ranges, ignored ranges, truncated streams, cancellation during a
stalled request, checksum failure, restart verification, concurrent write guards and
symlinks. The catalog is tested against the 500 MiB download budget. See the official
[Hugging Face download documentation](https://huggingface.co/docs/huggingface_hub/package_reference/file_download)
for the distinction between a pinned revision, ETag and content hashes.

## Vision adapter input contract (2026-09-24)

The recognition adapter uses the existing OpenAI-compatible `Request::Vision` path:
user-message `image_url` with inline PNG and `detail=high`, plus JSON output. This
shape and the `deepseek-flash` candidate were rechecked against the official
[vision guide](https://api-docs.deepseek.com/guides/vision/). Account/model access has
not been exercised. The application keeps its stricter 32 MiB request-body limit.
Canonical pages are decoded within existing pixel/allocation budgets and resized to
at most 2048 pixels on either axis for this initial overview adapter. Small-text crop
recognition remains required for quality acceptance; overview success is not proof
that dense or vertical text is complete.

The adapter rejects truncated responses, duplicate region IDs/order, empty text,
unknown region categories, noncontiguous reading order and out-of-bounds geometry.
Accepted coordinates map back to canonical pixels. It returns request-local IDs;
the result-publishing service assigns durable IDs and reconciles manual edits.
It produces no masks, cleanup, translation or lettering, and enables no processing
button until the remaining full-pipeline capability gates are satisfied.

## Native CPU experiment (2026-09-24)

`crates/manga-inference` contains an opt-in (`--features onnx`) Rust worker probe.
It loads an explicit native ONNX Runtime 1.22 library through `ort =2.0.0-rc.10`;
there is no system-runtime discovery, Python invocation or automatic download.
Packaging/distribution of that library and worker is still pending.

The second experimental catalog artifact is
[TareHimself/comic-text-mask](https://huggingface.co/TareHimself/comic-text-mask):
`model.safetensors`, revision `ebd37f1a9ae9298519678fb96a796573fb602977`,
57,377,484 bytes, SHA-256
`ab28dd8450462c4f87ddfd05d13601813bb90414f26bea5590bf6a0a7540f988`.
The repository declares MIT. The pinned ResNet18/U-Net architecture is assembled
from standard ONNX operators locally after checksum verification. RGB crops are
black-letterboxed to 384 pixels, normalized with ImageNet mean/std, thresholded at
0.5 and projected using the saved paste rectangle. No model Python code executes.
Safetensors and ONNX downloads have distinct cache/partial filenames.

A developer-only comparison used the same repository's pinned TorchScript artifact
(`model.pt`, SHA-256
`c1de7cd35c5cce1c5eca4e03696129dc9649db6ebe7d03fe1b995d7aca73e605`)
and PyTorch 2.7.1 CPU in an isolated temporary environment. On a synthetic 384-square
Latin/Cyrillic text crop, maximum probability error was 0.000005067 and binary
mask disagreements were zero. This checks the graph port, not manga quality.

Linux x86_64, Intel i7-3770K, two intra-op threads, release build, one isolated run:

- Mask: 593 ms cold initialization, 696 ms inference, 229,728 KiB peak worker RSS.
- LaMa: 8,564 ms cold initialization, 7,672 ms inference, 821,348 KiB peak worker RSS.
- Both model files together: 265,422,300 bytes, below the 500 MiB download cap.
- Cleaned PNG retained every decoded pixel outside the binary mask exactly.

These are single-crop smoke measurements, not page p95 or process-tree acceptance.
The LaMa output contract is float RGB in 0–255, not 0–1. Visible glyph remnants
remain on this synthetic fixture; mask margins/projection and cleanup quality need
further work. Do not enable automatic processing based on this experiment. Real
page quality, cancellation, bundled runtime size and all release OS/architecture
checks remain open. Release distribution must retain third-party runtime/model
license notices alongside this project's MIT license.

### P09 isolated worker and synthetic smoke follow-up

- Added bounded JSON IPC, SHA-256 checks at asset decode, create-new PNG outputs,
  one-worker admission and explicit process kill/reaping on cancellation or timeout.
  Cancellation tests require the process to disappear within one second; the next
  queued operation cannot start until the prior child is reaped.
- Region masks are unioned into canonical page coordinates. A configurable 0–8 px
  glyph margin never fills an empty prediction or an entire detection rectangle.
- Twelve native crate unit tests and a real CPU smoke test passed. The committed
  synthetic Latin/Cyrillic fixture exercises segmentation, cleanup, original-file
  preservation, exact pixels outside the mask and rejection of output overwrites.
  A two-pixel margin removed the obvious glyph remnants on this fixture; this is
  not representative-page quality acceptance.
- Repeat with `MANGA_RUNTIME`, `MANGA_MASK_MODEL`, `MANGA_LAMA_MODEL` pointing to
  explicit verified local artifacts, then `cargo test --offline --release
  --manifest-path crates/manga-inference/Cargo.toml --all-features -- --include-ignored`.
  Normal tests neither download weights nor require a runtime. Desktop wiring and
  installed-platform tests remain pending.

## Koharu reuse decision (2026-09-24)

User-provided upstream: [koharu-rs/koharu](https://github.com/koharu-rs/koharu),
reviewed at `c697b31eb1de016d9272743a2973f0e6a67eae6c`.
Use its existing manga implementations as the reference before writing more adapters.
The source is MIT OR Apache-2.0; adapted code uses MIT with its full copyright/permission
notice retained in `third-party/koharu-LICENSE-MIT` and included in desktop resources.
The application is now MIT-licensed by the user's subsequent decision; upstream notices remain intact.

The examined LaMa adapter uses `mayocream/lama-manga`, pinned upstream revision
`f91c85b26913b3e83f9877867b4c336da3675238`, file `lama-manga.safetensors`.
It is a manga-finetuned Big-LaMa checkpoint, not the same artifact as our experimental
`Carve/LaMa-ONNX`. The mask adapter uses `mayocream/manga-text-segmentation-2025`,
revision `efd866e3ac6595ea20722f35ae343c403056ba76`; it is different from our
experimental TareHimself ResNet18 mask. Do not substitute weights without changing
architecture/preprocessing and verifying their own terms and CPU resource budget.

Inspected upstream components:

- `koharu-ml/src/lama`: native model, symmetric padding, image/mask normalization,
  crop/resize orchestration and exact unmasked composition.
- `koharu-ml/src/manga_text_mask`: normalization, output probabilities, threshold,
  gap closing, hole filling and glyph dilation.
- `koharu-pipeline/src/stages/inpainting`: model lifetime and scene publication.

Already adapted symmetric border padding from the LaMa processor. Small crops keep
native resolution; the fixed-size ONNX experiment pads rather than magnifying them
into a black letterbox. Thin mask pixels survive downsampling; final composition
still preserves every original pixel outside the canonical mask. The current worker
still uses its existing experimental weights: this change does not claim to integrate
Koharu's native model or its whole pipeline.

Directly depending on the current `koharu-ml` crate also introduces its unconditional
Torch, llama, diffusion and runtime dependencies. Prefer extracting/adapting the
needed components with attribution and the existing worker boundary. Next compare
Koharu's manga-specific cleanup checkpoint/runtime and crop orchestration before
freezing the default model; reuse its lettering work when implementing that stage.

## Lettering implementation checkpoint (2026-09-25)

The isolated native worker now supports deterministic horizontal lettering with
rustybuzz shaping, fontdue rasterization and Unicode line-break opportunities.
The font-size fitting helper is adapted from Koharu at the pinned revision already
recorded above; its MIT notice remains included. Embedded DejaVu Sans is accompanied
by `third-party/DejaVu-LICENSE`. No ONNX runtime or downloaded weights are loaded for
this operation, and no local LLM is involved.

Each region receives centered black text with a one-pixel white outline. Layout
fits within the supplied rectangle, saves the exact font hash/size/line height, and
fails on missing glyphs or overflow rather than clipping text. Region count, text,
image, request and response sizes are bounded. The renderer preserves every pixel
outside the supplied regions and never modifies its input asset.

Russian/Latin synthetic rendering and the real worker protocol are verified. This
is not yet a general typography acceptance: CJK font fallback, mixed bidirectional
text, vertical layout, bubble-aware shape fitting and representative page quality
remain unverified/unsupported. The current rectangular layout is a baseline.

The application can execute durable `manga_lettering` jobs after current translation
and cleanup results exist. It fingerprints both the cleaned asset and region text/
geometry, rejects late edits, and publishes the derived image and chosen styles in
one checkpoint transaction. Recognition/translation test providers remain synthetic;
these checks do not make paid API calls. Native runtime packaging, full-pipeline
admission and automatic-processing UI remain separate unfinished work.

## Native resource packaging (2026-09-25)

`npm run manga:prepare` builds the Rust worker and prepares `src-tauri/manga-runtime/`.
Tauri release builds run it before the frontend build and include this directory as
resources. For development, run preparation once before `npm run tauri dev` and again
when changing native worker sources. Model weights are never included by this script.

The CPU archives are pinned to [ONNX Runtime 1.22.0](https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0)
in `scripts/manga-runtime.json`, with independently recorded SHA-256 for Linux x64,
Windows x64 and macOS x64/arm64. The script checks the archive before extraction,
builds for `TAURI_ENV_TARGET_TRIPLE` (or the Rust host), copies only runtime libraries,
and includes upstream LICENSE/ThirdPartyNotices plus font/Koharu/project notices.
`MANGA_RUNTIME_CACHE` optionally selects a build cache; it is not a user setting.

The generated manifest records target and file hashes. The desktop preflight rejects
missing, modified or wrong-target packs without loading native code into the app.
Native preparation verifies library loading when building for the host. Linux x64
preparation was executed successfully; the resource directory is approximately 25 MiB.
Other target archives were inspected but not compiled or executed on their platforms.
This does not establish release signing, minimum-OS or cross-platform acceptance.
