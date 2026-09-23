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
