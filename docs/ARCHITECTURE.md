# Architecture

Updated 2026-09-24 after removal of the superseded backend.

## Boundaries

The React workspace calls typed Tauri commands through `src/shared/api/transport.ts`.
Rust request/response types in `app/` generate `src/shared/contracts/generated.ts`.
`AppContext` owns the project manager, active job registries, edit previews and
assistant service. Commands adapt IPC; application services implement operations;
repositories and transactions enforce persistence rules.

- `project/`: inspect/import/create/open, versioned manifests, safe archives,
  deletion lifecycle and project leases. Imported content is self-contained.
- `storage/`: schema version 1, structural source blocks, immutable translation
  revisions, glossary, settings, contexts, assistant messages and durable jobs.
- `application/`: book translation, language repair, reference import, metadata,
  editing/search, export, glossary correction, provider profiles and assistant.
- `jobs/durable.rs`: ordered stages, checkpoints, cancellation, recovery and events.
- `ai/`: provider-neutral requests and OpenAI-compatible chat-completions transport.
- `book/` and `export/`: format parsers and writers shared by application services.
- `assets/`: immutable content-addressed assets and the `bookasset` URI protocol.
- `models/`: download/verify/pause/remove model artifacts. Automatic manga processing
  is still pending; model download support does not imply a working manga pipeline.

The old `session`, `state`, `translator`, `orchestrator`, assistant tool loop,
command operations and thread-based job runners are removed. No active command
opens `progress.db`. `project/reset.rs` remains an explicit one-shot cleanup utility
for the already-authorized legacy reset, not an automatic startup migration.

## Storage and identity

App-wide settings and provider credentials live in `settings.db`. Each project has
`projects/<id>/project.json`, `project.db`, and content-addressed assets. Manifests
carry a format version and project kind; unsupported/obsolete manifests are rejected.
The source/target language pair is chosen when the project is created and cannot be
changed by project settings or assistant actions.

Stable IDs identify projects, chapters and blocks. Revisions cross IPC as decimal
strings. Mutations check captured revisions; a late model response cannot overwrite
newer manual work. Translation edits publish a new revision and retain historical
results. Registered asset IDs resolve inside the selected project's asset directory.
Book text that resembles an image marker remains text; export accepts explicit
text/image blocks and no longer has a legacy string-markup branch.

## Book execution

Import handles TXT, FB2, EPUB, PDF and supported ZIP containers. EPUB/FB2 preserve
structural illustrations. PDF text uses available extraction backends, with outline
fallback for chapter boundaries and best-effort first-page JPEG cover extraction.
Unrecognized text headings retain the source as one chapter with an import warning;
there is no automatic paid request to infer a delimiter during import.

Translation selects eligible chapters in source order, then applies the requested
maximum chapter count. Existing translations are skipped unless explicitly forced.
Optional glossary extraction, translation and rolling-context generation are durable
stages. Each translation request carries bounded segments, matching glossary entries,
book/chapter instructions and preceding continuity. Language repair targets the
problem title/lines; successful unaffected output is retained.

A reference supplies the full translation of aligned untranslated chapters and keeps
its reference origin. It is not merely a preview. A mapping/import never silently
replaces existing authored work. The chapter view derives completion/error/language
flags separately from origin and review state.

Glossary target changes save the term and offer a confirmation modal with local
candidate counts and a batch limit. Only confirmation starts a correction job.
The correction prompt includes existing translated fragments, original context,
old/new terminology and matching glossary entries. Other lines remain unchanged;
summaries are corrected and closing excerpts rebuilt. Checkpoints support resume.

Jobs persist their selections, provider settings and input fingerprints. Startup
marks interrupted runs; resume validates checkpoints and inputs. Job events identify
the project and run. The job panel derives remaining time from stage measurements.

## Assistant and UI

The assistant is a right-side panel with persisted project history and bounded
chapter context. Its application service proposes supported actions, captures
revisions and applies confirmed changes through the same services as the editor.
Current actions cover book/chapter instructions, glossary terms, current-chapter
literal replacements and explicitly bounded translation batches. This is a constrained
proposal system, not the removed unrestricted legacy tool loop.

The book overview owns metadata, cover, instructions, progress and batch controls.
The reader edits structural text; chapter filters collapse and search opens in the
sidebar. The assistant and search retain their local state while hidden. Exports
read a consistent snapshot, enforce the chosen incomplete-book policy and publish
atomically without overwriting an existing destination.

## Verification and remaining work

Use `REFACTORING_STATUS.md` for execution evidence. Offline tests cover provider
payloads, persistence guards, checkpoint resume, parsing and export. The cross-service
book test imports a structural EPUB, translates a bounded batch with a fake provider,
edits, recreates the project manager, searches, exports and imports the output again.

Native Tauri acceptance, real-provider quality and cross-platform packaged execution
remain separate gates. Manga automatic processing follows book work and backend
cleanup; no phase is declared complete solely because compilation succeeds.
