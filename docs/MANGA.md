# Manga

## Import and navigation

Create a **Manga** project from CBZ/ZIP or an image folder. Folder/volume names and
pages use natural ordering. Images are normalized for orientation; thumbnails are
stored independently. Identical images can share asset bytes while retaining separate
page identities. Project source/target languages are fixed at creation.

RAR/CBR import and manga-specific EPUB import are not implemented. Extract unsupported
archives to an image folder before importing. Imported projects keep their own assets,
so navigation does not require the source archive to remain available.

The workspace provides volume filtering, virtualized thumbnails, page navigation,
zoom, panning, region overlays and original/translated views. A page without a render
is not considered translated. A page without recognition is not treated as an empty
page. Selecting a page does not start recognition or translation.

## Processing prerequisites

Settings supplies API profiles for recognition and translation; unassigned roles use
the shared default configuration. Recognition needs a provider/model that accepts
image input. The local preflight validates configured access, model installation and
native resources; it does not contact the provider or prove OCR quality.

Install both catalog models from Settings and ensure the packaged native worker/runtime
is present. Development setup is described in [Native runtime](MANGA_RUNTIME.md).
Import and viewing remain available without these processing prerequisites.

## Automatic batches

Start a requested number of eligible pages from the current page, within the selected
volume or all volumes. The job freezes settings, both API profiles and local component
versions. It finishes all stages for one page before moving to the next:

1. **Recognition:** cloud vision returns text, geometry, category and reading order.
   Coordinates are mapped into the normalized original image space.
2. **Translation:** cloud text generation translates ordered regions by ID, using
   project languages and locally matched glossary terms. Manual translations are kept.
3. **Masks:** the local segmentation model identifies text pixels.
4. **Inpainting:** the local cleanup model generates a cleaned image.
5. **Lettering:** local shaping/layout draws the translated text and publishes a render.

Each stage has its own durable result, fingerprint and checkpoint. Cancellation keeps
committed work. Resume reuses current results and recomputes invalidated stages.
Original pages are immutable; masks, cleaned pages and rendered pages use new asset IDs.
The pipeline and worker interaction are shown in [Architecture](ARCHITECTURE.md).

## Region editing and rebuilds

Open **Regions**, select a frame, drag to move it and use its lower-right corner to
resize. Geometry stays in original pixels at every zoom. Choose horizontal lettering
or vertically oriented lettering rotated by 90 degrees in the inspector.

**Apply** saves pending edits and starts a local masks → inpainting → lettering rebuild
using the original page and saved translations. It does not request recognition or
translation again. Explicitly changed rectangles can be used as full cleanup masks.
**Cancel** discards pending edits. Enlarging a region can resolve layout overflow.

Recognition reruns reconcile existing regions and preserve manual text where possible;
unmatched edited regions remain available for review. Result freshness is separate
from execution status and manual-review flags. Geometry changes invalidate local image
stages; text/style changes invalidate their dependent outputs.

## Errors and output

Jobs distinguishes text overflow, missing font coverage, worker timeout and other local
processing failures. Preflight reports missing API configuration, weights or native
resources before a batch starts. A verified download means the artifact matches the
catalog; it does not guarantee good segmentation, translation or typography on every page.

Portable `.bcproj` archives include pages, regions, edits and versioned assets.
Rendered manga export to CBZ/EPUB is currently disabled. Complex SFX recreation and
arbitrary handwritten lettering are not guaranteed by the existing layout engine.
Check the resulting pages visually before treating a translation as finished.
