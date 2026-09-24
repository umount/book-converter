# Manga workflow requirements

Updated 2026-09-24. This is the target workflow; automatic processing is not implemented.
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

Masks must identify text pixels without erasing whole rectangular bubbles or panel art.
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
from a text rectangle. Full-processing UI remains unavailable pending the remaining
capabilities and real model quality verification.

The page workspace can inspect saved recognition results with selectable overlays
and a collapsible right panel. It distinguishes unrecognized, empty and stale OCR
and shows preserved manual-text flags. Region text is currently read-only in the UI.
Opening or selecting a page never starts a paid recognition request.

The workspace now exposes a local processing preflight with stage-specific reasons.
Opening or refreshing it never uploads pages, downloads models or creates jobs.
Configured recognition means the local profile validates, not that provider image
access or OCR quality has been verified. Downloaded weights do not bypass missing
segmentation, cleanup runtime or lettering capabilities.
