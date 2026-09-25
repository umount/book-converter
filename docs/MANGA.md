# Manga workflow requirements

Updated 2026-09-25. Automatic processing through lettering is implemented and tested
with synthetic API replies plus real native image stages; product acceptance remains open.
Book scope is frozen and superseded backend cleanup is complete. Manga work is active. See [execution status](REFACTORING_STATUS.md)
and [model/platform assessment](MANGA_TOOLING.md).

## Domain and workspace

Manga is an explicitly selected project kind. Illustrated books remain books. Volumes,
pages, regions, masks and versioned renders are separate from book chapters/blocks.
Identical image bytes can share an asset while remaining distinct pages with distinct IDs.
The language pair is fixed at creation. RTL controls reading order, not image mirroring.

Import must preserve natural volume/page order. Initial sources are CBZ/ZIP, supported
RAR/CBR extraction and image folders. If no RAR extractor is available, an extracted
folder remains an import option. Password/corruption/unsupported-format errors must be
clear. Archive traversal, links, expanded size/count and image dimensions are validated.
EPUB manga import is a later adapter; a spine item is not necessarily one image/page.

The workspace needs volume/page navigation with virtualized thumbnails, a zoomable
canvas, original/translation/comparison views and a switchable region inspector or
assistant panel. Before a render exists, say that translation is not ready. Do not
infer that a page has no text before recognition. Load full-resolution images only
where necessary, with bounded memory and neighbor prefetch.

## Automatic pipeline

Recognition → region translation → generated masks → cleanup → lettering → review/export.
Persist logical stages independently even when a provider combines requests. Recognition
produces region IDs, geometry, source text, category and reading order; translation
returns text keyed by those IDs, not invented geometry.

Use cloud recognition/translation and lightweight local cleanup/lettering. Required
model weights are downloaded on demand, including Hugging Face sources when selected;
do not bundle the full weights by default. Windows, macOS and Linux are required,
with no mandatory GPU. Actual model choice depends on tooling/quality/resource gates.
There is no manual/model-free processing fallback. Import/viewing work without a ready
processing stack, but processing preflight blocks until all capabilities are available.

Automatic masks identify text pixels without erasing whole rectangular bubbles or panel art.
Explicitly moved/resized regions use the user-selected rectangle as a full cleanup mask.
Coordinates use EXIF-normalized canonical pixels with inverse crop/resize mappings.
Lettering records font, size, alignment, line spacing and stroke. Overflow requires
review; never silently clip it. Complex SFX/handwriting matching is not promised initially.

## Revisions and selective reruns

Execution status, result freshness and human review are separate. Originals are immutable;
every derived image/mask has a separate asset identity. Never overwrite bytes behind an
unchanged URL. Publish only complete results whose input revisions still match.

- Translation/font edits invalidate lettering, not recognition/cleanup.
- Mask edits invalidate cleanup/lettering, not translation.
- Source-text edits invalidate dependent translation/render.
- Geometry edits invalidate masks/cleanup/layout.
- Recognition reruns preserve old revisions and reconcile edited text.
- Earlier dialogue changes may stale later context; never silently start paid reruns.

Start with bounded execution and sequential contextual translation. Resume stages from
checkpoints; support cancellation, finite retry and stage/page progress with measured ETA.
Manual region/mask drawing is outside this scope; failed automatic results need review/retry.

## Portability and acceptance

Project archives preserve kind, volumes, page identities, original assets, regions,
masks, edits and versioned results in a consistent snapshot. Credentials and model
weights are not project content. Manga output is ordered CBZ/EPUB with explicit policies:
ready results only or originals for unfinished pages. Do not silently drop pages.

Acceptance must verify multi-volume ordering, offline viewing after the source archive
is removed, bounded image memory, correct geometry after crop/resize, selective reruns,
stale-result rejection, restart/resume and portable archives. Validate actual Tauri image
loading and model quality on representative samples. Manga assistant actions must call
manga services and must never dispatch book chapter operations.

## Implemented recognition backend

An explicit bounded recognition-stage job now persists page geometry and OCR text,
with cancellation and checkpointed resume. It requires the manga recognition profile
and rejects unsupported stages; it does not run the complete translation pipeline.
Reruns retain matched manual text and preserve unmatched edited regions for review.
Original assets remain immutable. No automatic mask or inpainting result is generated
from a text rectangle. The automatic job composes this stage with translation,
masks, cleanup and lettering.

The page workspace can inspect saved recognition results with selectable overlays
and a collapsible right panel. It distinguishes unrecognized, empty and stale OCR
and shows preserved manual-text flags. Region text is currently read-only in the UI.
Opening or selecting a page never starts a paid recognition request.

The workspace now exposes a local processing preflight with stage-specific reasons.
Opening or refreshing it never uploads pages, downloads models or creates jobs.
Configured recognition means the local profile validates, not that provider image
access or OCR quality has been verified. Downloaded weights do not bypass missing
segmentation, cleanup runtime or lettering capabilities.

## Implemented dialogue translation backend

The explicit translation stage uses the project's manga translation API profile,
fixed languages, ordered OCR regions and locally matched glossary terms. It preserves
manual translations, publishes a complete versioned result atomically and resumes
without translating already completed pages again. Malformed or stale replies cannot
replace region text. Qwen/local LLM deployment is explicitly outside this implementation.

Local mask and cleanup stages now use the isolated native worker through the durable
job runner. The backend resolves installed catalog models and packaged native files;
results are revision-checked immutable assets. Lettering uses the cleaned result and
current translated regions, validates font coverage/overflow and saves its chosen style.

## Automatic batch controls

A compact control starts a requested number of unfinished pages from the current page
within the selected volume (or all volumes). One durable run freezes both API profiles,
settings and native versions. Processing finishes each page before moving on, and
resume reuses successful current results without repeating API calls. Stale results
are recomputed, preserving attempt history. Nothing starts when a page is opened.

Preflight requires valid profiles, downloaded mask/cleanup models and an intact native
resource pack. `npm run manga:prepare` prepares development resources; release builds
run it automatically. The manga viewer can switch between the current translated
render and its immutable original. Job details show volume, page, stage and page count.

Live API quality, native GUI interactions, representative manga typography, target
resource budgets, review/export and non-Linux packaged builds still need acceptance.

## Editing recognized regions

Open **Regions**, drag a frame to move it, and drag its lower-right corner to resize.
Coordinates stay in original image pixels at every zoom. Select horizontal lettering
or vertical lettering rotated 90 degrees in the inspector. **Apply** saves pending edits
and runs a local masks → inpainting → lettering job from the immutable original page,
using existing translated text; it does not call recognition/translation APIs again.
**Cancel** discards pending edits. Enlarging a narrow region can resolve text overflow.
Manual rectangle cleanup and text direction survive subsequent local rebuilds.

Worker overflow, unsupported glyphs and timeout errors are reported separately in Jobs.
Old failures recorded as `mangaLocalProcessing` cannot recover details retrospectively.
