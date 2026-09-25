# Books

## Import and project identity

Create a **Book** project from TXT, FB2, EPUB, PDF or a supported book inside ZIP.
Confirm the source and target languages before creation; the language pair is fixed
for that project. Import is local and does not request AI processing.

The importer produces ordered chapters and stable text, caption and image blocks.
EPUB/FB2 images remain structural content, including repeated illustrations. FB2
sections without numbered headings are retained. If text headings cannot be split,
the import keeps the content as one chapter and reports a warning. PDF handling uses
available extraction backends, outline-based boundaries where applicable, and
best-effort first-page JPEG cover extraction; scanned documents are not a guaranteed
text source.

## Overview and translation

Overview contains the book title, author, annotation, cover, shared instructions and
batch controls. Source metadata, generated metadata and manual overrides are stored
separately. Unknown authors are not inferred from chapter prose. Generating metadata
or an annotation is an explicit provider-backed job.

Choose a positive chapter count and start translation. Selection follows source order
and skips ineligible/already translated chapters unless repeat processing is requested.
Optional glossary extraction precedes translation for each selected chapter.
Processing stops when the selected batch is finished.

Requests carry stable text-segment IDs, relevant glossary entries, book/chapter
instructions and preceding story context. The backend retries unresolved segment
output and repairs affected foreign-language lines. Images retain their original
position and bytes. A successful translation and its checkpoint are committed together;
context generation is a separate resumable stage.

Metadata output is checked for incompatible writing systems where the target language
is supported by the script checker. The application attempts repair before saving;
unresolved output produces a language error. This does not replace editorial review
or distinguish every pair of languages using the same alphabet.

## Glossary

The glossary stores source terms, translations, categories, pin state and frequency.
Its header shows the project-wide total; search and pinned filters show the matching
count separately. Pagination limits the displayed batch, not the glossary's size.

Extraction has a chapter batch size but no cap on the number of returned terms.
It accepts the model's structured term records, merges entries by source spelling,
and preserves existing targets. Terms that do not literally occur in the source are
kept with zero frequency. The model must still return complete, parseable JSON.
Repeated extraction can include chapters already processed at their current revisions.

Saving a changed target offers correction of existing translations. The preview gives
local candidate counts; confirmation starts a bounded correction job. It supplies the
old/new terminology, translated fragments, original context and relevant glossary
entries. Unaffected text stays intact; affected summaries and closing excerpts are
updated with the corrected results.

## Reading, editing and references

Read source and translation side by side, edit text and translated titles, or open
chapter instructions from the reader toolbar. Search and replace use the sidebar
(Ctrl/Cmd+F and Ctrl/Cmd+H). Saves use expected revisions and cannot silently overwrite
newer edits. Translation changes publish a new revision rather than rewriting history.

Reference import aligns chapters and can supply their full translated content.
Mapping does not silently replace an existing authored translation. Origin (generated,
reference or manual), execution errors, language issues and review state are separate
properties. Changing dependencies can mark results for review while keeping them
available to read and edit.

## Jobs and recovery

Jobs shows the current chapter/stage, completed work, ETA and structured failure.
Active work appears before completed history. Failed, cancelled and interrupted jobs
can be resumed; valid successful checkpoints are reused. Opening a project never
resumes paid work automatically. A failure does not discard previously committed
chapters or successful stages.

Provider connection/HTTP failures, malformed segment output and malformed or truncated
glossary output are different conditions. Correct the relevant provider setting or
input before retrying when necessary. Details absent from an older saved error cannot
be reconstructed retrospectively.

## Export

**File → Export** supports TXT, FB2 packaged as `.fb2.zip`, EPUB and PDF. The default exports ready translated chapters and image-only chapters, skipping unfinished text chapters and untranslated empty headings. The cover is preserved. You can optionally include original text for unfinished chapters; translating the entire book is never required. Export reads a consistent snapshot, preserves
structural images in formats that support them, and refuses an existing destination.
TXT is a text-only format.

Use `.bcproj` for continuing work on another installation. It contains the manifest,
database snapshot and referenced assets, including edits and processing records.
Configure API credentials separately on the destination installation.

See [Architecture](ARCHITECTURE.md), [Settings](SETTINGS.md) and [Assistant](ASSISTANT.md).
