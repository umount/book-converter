# Refactoring execution status

## Current phase

P00: completed. Baseline and deterministic fixtures.
P01: completed. Typed contracts, composition boundary and frontend test harness.
P02: completed. Versioned repositories, immutable assets and domain result persistence.
P03: lifecycle wired to the new UI; explicit legacy reset executed. Native acceptance remains.
P04: in_progress. Durable execution and structured provider transport implemented.
P05: in_progress. Translation, context, editing, reference, export and metadata backend implemented.
P06: in_progress. New shell, wizard, structural editor and supporting screens wired; native acceptance remains.
P07: in_progress. Book assistant, guarded proposals and persisted history implemented.
P08: partial. Import/read-only page workspace available; model processing not implemented.
P09–P13: pending.

## Baseline (2026-09-23)

- Source baseline: d636c5d. Planning documents were uncommitted at implementation start.
- Host: Linux x86_64, Rust 1.94.0, Node 18.20.4, approximately 19 GiB RAM.
- Required releases: Windows x86_64, Linux x86_64, macOS arm64/x86_64.
- Initial release floors: Windows 10 22H2, macOS 13, Ubuntu 22.04-compatible Linux.
  These are product targets, not verified packaged compatibility. Tauri prerequisites
  were checked at https://v2.tauri.app/start/prerequisites/; native model/PDF runtimes
  must be tested against these floors in P10/P12. This host does not establish
  Windows/macOS compatibility or the 8 GiB CPU budget.
- `npm run build`: exit 0, TypeScript and Vite production build pass.
- `cargo check --offline --manifest-path src-tauri/Cargo.toml --all-targets`: exit 0.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib`: exit 101,
  239 passed, one failed because session::tests::db_path_for_project_ends_with_progress_db
  writes to the sandbox's read-only application data directory. Rerun with an isolated
  XDG_DATA_HOME; never use actual project data for test writes.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: exit 1, existing
  formatting drift in book/blocks.rs, book/epub.rs, book/mod.rs, book/parser.rs,
  paths.rs and state/blocks.rs. Keep baseline cleanup separate from domain changes.
- Clippy baseline: exit 0 with --all-targets -- -D warnings.
- Isolated XDG_DATA_HOME=/tmp/book-converter-refactor-tests library rerun: exit 0,
  all 240 tests passed.
- Fixture regeneration: identical manifest hashes on two consecutive runs.

## Implemented

- Deterministic fixture generator in scripts/generate-fixtures.py.
- Synthetic Chinese TXT, FB2 reference, structural EPUB and naturally unsorted
  two-volume CBZ. Manifest records expected counts and SHA-256 hashes.
- Repeated image bytes intentionally represent distinct occurrences/pages.
- No source samples, credentials, settings or existing projects modified/deleted.

## Next executable step

User priority update (2026-09-24): **books → legacy removal → manga**.

1. Close the remaining book workflow and fidelity gaps, including FB2 inline
   illustrations, assistant scope and book-specific acceptance. Verify import →
   metadata → bounded translation → edit → search → export and native save/close
   behavior where the host permits it. Keep language pairs immutable.
2. Delete superseded code and IPC, preserving still-needed book behavior through
   the new services. Verify regressions and replace outdated architecture docs.
3. Only then resume automatic manga recognition, cleanup, lettering and acceptance.

Original phase numbering does not override this priority. Existing manga functionality
must remain usable through shared changes, but new manga features are deferred.

## Evidence rules

No OCR/model benchmarks or native GUI smoke tests have run. No phase is complete
without its acceptance evidence. Automatic manga processing has no manual/model-free
fallback. Keep commits scoped; push only on user request.

## P01 progress

- Added canonical Rust project, error, selection, event, block and manga capability
  types; ts-rs =12.0.1 generates TypeScript, UUID v4 supplies project identity.
- Added versioned manifest parsing and read-only project_inspect_manifest IPC.
  Missing/unknown format versions and missing kinds are rejected explicitly.
- Revisions serialize as decimal strings to avoid JavaScript precision loss.
- All six manga stages must be available; no manual fallback satisfies preflight.
- New app contract tests: 5 passed; manifest tests: 2 passed. Full isolated library
  regression: 252 passed, 0 failed. Strict all-target Clippy: exit 0.
- `npm run build`: passed with generated contracts.
- The remaining P01 work listed at interruption was completed on 2026-09-24;
  see the continuation evidence below.

## P02 prototype

- Version-1 SQL schema and transactional text compare-and-swap added for exercising
  contracts. Four storage tests passed: domain isolation, stale writes, broken asset
  references and duplicate image page identity.
- Content-addressed AssetStore publication/deduplication test passed. Image MIME
  and dimensions are validated; publication never replaces existing content.
  Assets no longer import a constant from commands. No UI/persistence cutover yet.
- This is deliberately not a completed P02; shared repositories and complete domain
  result persistence still need work before project lifecycle replacement.

## Recovery and P01 continuation (2026-09-24)

- Recovered the interrupted working tree; no source samples or application data changed.
- Recovery verification: 253 Rust library tests passed; frontend production build and
  strict all-target Clippy passed. Saved the P02 prototype as `a77231f`.
- Added domain ID wrappers and exact project/book/manga request DTOs, page/chapter
  views, shared glossary/job/assistant/settings requests, and generated TS exports.
  DTO declaration does not register an IPC command; implementations remain phased.
- Added a Tauri-independent AppContext/ProjectService composition boundary. The
  existing manifest command now delegates through managed, injectable services.
- Event payloads are flattened to the documented top-level type/payload envelope.
- Stable selections reject unknown/duplicate IDs and reversed ranges, then resolve
  in source order. Revision parsing rejects noncanonical or overflowing values.
- Added an injectable frontend invoke/listen boundary and a dependency-free Node
  test harness using the existing TypeScript compiler. Tests cover structured error
  propagation, project/version isolation, and disposal during async registration.
- Final P01 evidence: 257 Rust library tests passed; contract generation/check,
  npm test, npm run build and strict all-target Clippy passed. ts-rs reports that
  deny_unknown_fields has no TS representation; serde still enforces it at runtime.
- P01 is complete as a contract/composition milestone, not a domain/UI cutover.

## P02 repository/protocol slice (2026-09-24)

- Added a domain-scoped ProjectRepository: atomic chapter/block insertion, consistent
  source chapter snapshots, revision-checked text updates, manga volume/page insertion
  and ordered page reads. Page dimensions must match registered immutable assets.
- Removed the standalone untyped text-update prototype. Missing entities, wrong
  domain, invalid image edits and stale revisions now have distinct AppError codes.
- The image protocol reads through AssetStore; existing file, asset-directory and
  project-directory symlinks are rejected without creating paths during reads.
- Added NOT NULL to text primary keys and SQL guards against changing project kind,
  asset metadata, or original page pixels/dimensions after insertion.
- Evidence: 260 Rust library tests passed; strict all-target Clippy passed.
  After the final SQL guard changes, all 7 storage tests passed again, including
  rollback on missing assets, source block order, kind isolation and immutable data.
- P02 remains in progress. Shared settings/glossary/job repositories, translation
  and manga result persistence, and complete schema invariant coverage remain.
- No user data reset, GUI cutover, paid model call or remote push was performed.

## P02 persistence completion (2026-09-24)

- Added revision-checked settings, glossary, assistant transcript and durable job
  repositories; job snapshots contain profile IDs and resolved choices, not credentials.
- Book translation publication validates the exact text block set, source/settings/
  glossary revisions and prior translation revision in one transaction. Images stay
  structural. Contexts belong to a specific translation revision.
- Manga result publication validates input versions and immutable asset references;
  edits preserve manual flags, invalidate dependent results and reject stale updates.
  Added region, mask, review and explicit reference-mapping persistence.
- Strengthened integer revision constraints, source-block guards and indexes.
- Targeted storage run: 13 tests passed. Clippy found one test assertion style issue,
  corrected before commit. Final regression/Clippy is deferred to the next major
  boundary per the user's request to avoid rebuilding every intermediate state.

## P03 lifecycle backend (2026-09-24)

- Added staged source inspection, choice resolution, idempotent creation, validated
  catalog/open, cancellation and deletion barriers. A project lease covers the
  lifetime of network work and DB writes; deletion cancels and waits for leases.
- Added source normalization for existing book parsers and CBZ/ZIP manga fixtures.
  EPUB short asset IDs are remapped to full content hashes; repeated image occurrences
  retain separate block/page IDs. No AI call is made by inspect/create/open.
- Archive export uses VACUUM INTO plus registered immutable assets. Import rejects
  unsafe/duplicate entries, validates integrity/FKs and image hashes, generates a
  new project ID, and marks interrupted work explicitly.
- Added one-shot legacy reset with exact candidate enumeration and a mandatory
  quiescence callback. Removed automatic legacy cleanup from application startup.
  The reset is not invoked on actual user data yet; UI-key clearing belongs to cutover.
- 270 library tests and strict Clippy passed at this boundary, including real fixture
  import, archive roundtrip, duplicate images, cancellation and deletion waiting.
  The subsequently added reset test is included in the next combined regression.
- Commands are registered, but the old UI still uses its old command set until P06.
  RAR/folder ingestion and EXIF normalization remain P08 work, not claimed here.

## P04 runner and P05 translation/context boundary (2026-09-24)

- Added a shared HTTP provider with structured/text/vision/tool request shapes,
  bounded transport retries and response sizes, credential-free profile snapshots,
  and a global two-request limit. Wire reference: https://api-docs.deepseek.com/api/create-chat-completion/.
  No paid API request was made. Streaming and role-specific credential stores remain.
- Durable execution persists each domain result and successful step atomically.
  Cancellation drops pending requests; fingerprints and repository revisions reject
  late results. Resume skips matching successful steps. Startup marks running work
  interrupted without automatically repeating requests. Completion checks all expected
  stages, including jobs with no persisted steps.
- Structural book translation chunks Unicode text deterministically, validates exact
  segment IDs, repairs only missing/duplicate segments within a finite budget, and
  runs context generation independently. Images never enter the text protocol.
- Added run admission/cancellation, book/job IPC, translated chapter reads and
  revision-checked manual saves. Manual edits publish a new snapshot and preserve
  historical translations. Typed frontend API methods are ready for P06 integration.
- Evidence: 281 Rust library tests, strict all-target Clippy, contract generation,
  npm test and npm run build passed. Tests include fixture translation with repeated
  images, partial output repair, cancellation, late results, resume after context
  failure and stale manual-editor rejection. A SQL table typo in the new test was
  corrected before the final successful run.
- P04/P05 are not declared complete: streaming/role configuration and broader job
  acceptance remain; P05 metadata/glossary jobs, reference/edit tools and structured
  exports remain. The legacy UI/handlers are still present; reset was not executed.

## P05 structural editing continuation (2026-09-24)

- Added literal, Unicode-aware replacement preview across selected translated
  chapters. Previews are scoped to a project, expire after 30 minutes, and have
  explicit count/size bounds. Preview does not write domain data.
- Apply consumes the preview and saves every chapter in one transaction, checking
  source/settings/glossary/translation revisions. A conflict in a later chapter
  rolls back earlier writes. Text resembling regex replacement syntax stays literal.
- Added typed preview/apply and source-block editing IPC; source edits use the
  repository's existing revision guard and downstream invalidation.
- The replacement integration test passes, including stale-second-chapter rollback,
  project isolation, single-use previews, and literal replacement text.
- Follow-up evidence: all 4 durable runner tests passed, now including concurrent
  projects and deletion while a request is pending. Strict Clippy, regenerated
  contracts, frontend tests and production build passed after editing integration.
- Provider credentials are now profile-scoped. Resume compares the saved endpoint
  and profile ID against local configuration before loading its credential into the
  transport; an imported snapshot cannot redirect the default key. The dedicated
  credential-destination regression test passed. The legacy default key is used only
  by the default profile; custom profiles require their own credential setting.

## P05 export and reference boundary (2026-09-24)

- All four existing book writers now accept explicit structural text/image blocks.
  Legacy marker parsing is confined to the legacy body variant; new text that looks
  like a marker stays text. Typed exports read one SQLite snapshot, enforce the
  incomplete-translation policy, verify/copy immutable assets, and publish a complete
  file without replacing an existing destination or writing inside app storage.
- 14 export tests passed, including existing PDF checks and new EPUB/FB2 duplicate
  image occurrence checks, literal-marker text and failure without output publication.
  Explicit typed PDF invocation and platform/font coverage still need P12 validation.
- Added reference import, read and explicit stable-ID correspondence commands.
  Reference changes use a fingerprint guard, roll back invalid mappings and advance
  chapter revisions, rejecting already-running translations with old reference input.
  Reapplying the same correspondence does not advance revisions.
- The combined regression at this boundary passed all 287 Rust tests. Strict Clippy
  passed before the subsequent metadata/instructions additions.

## P05 metadata and chapter instructions (2026-09-24)

- Added an independent persisted metadata job and immutable metadata results. It
  uses a bounded source sample, rejects truncated/malformed responses, and validates
  source/settings versions before publication. Opening a project never starts it.
  Empty text layers do not trigger a metadata provider request.
- Metadata reads expose validity; source/settings changes make old results stale.
  Structured export uses current metadata and falls back to the project name when
  no current generated title exists. Credentials remain outside project persistence.
- Added revision-guarded chapter instructions and included them in translation
  prompts. Changes invalidate chapter/dependent results and stale editors conflict.
- Targeted metadata tests and the chapter-instruction regression passed. Final
  contract generation, frontend build and Clippy are recorded below after integration.
- P05 remains in progress: glossary extraction jobs, shared glossary/retarget UI
  integration, broader roundtrip coverage and the P06 workspace cutover remain.
  The new metadata table is part of the unreleased format-1 schema; no deployed
  project migration or user-data reset was executed.
- Final integration evidence: all 8 application-service tests and the new chapter
  instruction test passed; strict all-target Clippy, contract generation/check,
  frontend tests and production build passed. Clippy's test-module ordering finding
  was corrected before this successful run. No paid request, native GUI smoke test,
  user-data reset or remote push was performed.


## Fixed languages, glossary extraction and P06 state (2026-09-24)

- User clarified that a project chooses its source/target languages once during
  creation. Removed language changes from processing-profile update DTOs; added
  repository validation and one-way SQL language locks. Staging resolves both
  languages before publication. Catalog/open/archive import reject unconfirmed
  language choices. No language-scoped glossary or runtime retarget was retained.
- Added independent durable glossary extraction in bounded overlapping source
  chunks. Source terms must occur in the input, output size is bounded, and the
  result is saved atomically with the successful step. Existing targets/kinds/pins
  are preserved; occurrences are stored per processed chapter without double-counting
  on retries. Late source/settings/glossary results conflict instead of overwriting.
- Added paginated glossary reads and guarded create/update/delete, plus read-only
  project languages and editable provider roles. Profile updates invalidate derived
  results without changing languages; identical settings are a no-op.
- Added pure frontend WorkspaceStore and JobStore for P06. Stable IDs and request
  generations reject late project/chapter responses. Per-project jobs survive visible
  workspace switches; decimal revisions use BigInt, stale snapshots are ignored,
  and events during a read coalesce without polling unchanged work.
- Evidence: 289-test combined Rust regression passed before the added acceptance
  tests; subsequently all 19 glossary-filtered tests and the fixed-language test
  passed. These cover pins, atomic invalidation, stale edits, pagination and durable
  resume without repeating the first successful chapter. Strict all-target Clippy,
  generated TS contracts, frontend tests and production build passed.
- Frontend tests cover rapid project/chapter switching, large revision counters,
  interleaved background jobs and disposal. Stores are not yet connected to App;
  the legacy UI remains until P06 cutover. No user data reset or paid AI request ran.


## P06 frontend cutover and explicit legacy reset (2026-09-24)

- Replaced the old App, components, hooks, string-based reader, API/types and old
  localization dictionary. No legacy frontend fallback remains. Kept the reusable
  virtual list under shared/ui; new feature boundaries are projects/book/glossary/manga.
- New project library and staged wizard choose kind before source and fix languages
  at creation. Global target default only initializes the wizard. Opening projects
  does not trigger AI. Optional metadata is explicitly submitted once after create.
- Structural paired reader preserves image blocks. Serialized autosaves use translation
  IDs/revisions and fetch the newly saved snapshot before the next block edit. Typing
  during a request is retained; stale refreshes cannot overwrite a later edit. Conflicts
  preserve drafts and prevent navigation. Instructions/reference mappings flush on
  navigation, Ctrl/Cmd+S and native close; expected revisions/fingerprints are retained.
- Added overview/metadata, glossary editing and extraction, explicit reference mapping,
  guarded find/replace previews, book exports, archive import/export, provider defaults,
  English/Russian/Chinese UI and Ctrl/Cmd+K command palette. Jobs remain visible across
  projects and cancellation is independent of a conflicting editor draft.
- Manga has a paginated read-only image workspace and shared glossary/archive actions.
  The UI explicitly states that automatic processing needs configured models; no
  manual/model-free processing fallback or unimplemented assistant tools are advertised.
- Bare asset IDs resolve through the project's registered assets table, with safe filename
  checks. Manga page cursors span volume boundaries and reject another-volume cursors.
- Added a development-only memory fixture (`npm run dev`, `?preview=1`) for browser QA.
  It is explicitly labelled and excluded from the production bundle. Browser checks at
  the native 1100×760 window size covered layout, edit → chapter switch → return,
  instruction isolation/save across chapters, glossary creation and the command palette.
- Evidence: all 295 Rust library tests, strict all-target Clippy, generated contract check,
  frontend transport/state/editor tests and production build passed. New tests cover
  concurrent typing, conflict retention, stale refreshes, registered asset paths and
  page pagination. A full native Tauri interaction flow and paid AI calls were not run;
  P06 is not marked complete on browser fixture evidence alone.
- At the user's explicit request, inspected and removed exactly two legacy project
  directories with project/reset.rs through the explicit reset_legacy maintenance
  example. No Book Converter process was running. The app project directory is empty;
  settings.db and the existing external source were verified unchanged by SHA-256.
  A persistent one-shot marker prevents repeat deletion. No startup reset was added.
  One older manifest already pointed at a missing source before the reset.
- The maintenance tool is read-only without `--apply --app-stopped`; callers must first
  stop the application/writers. Legacy Rust handlers remain until application-service
  extraction/P12 cleanup; the new frontend does not invoke them.

## On-demand models and application identity (2026-09-24)

- Implemented global ModelManager and typed model_list/download/pause/remove commands.
  Weights are fetched only by an explicit action from a pinned Hugging Face catalog;
  they are not bundled with the app or downloaded during project creation/opening.
- The first catalog entry is an experimental LaMa ONNX cleanup artifact (198.4 MiB).
  Its commit, exact byte length and SHA-256 were checked against the public Hub API.
  Download acceptance does not imply inference, quality or platform acceptance.
- Added Settings controls with size, progress, pause/resume, removal and localized
  failures in English/Russian/Chinese. The cache survives app restarts and stays
  independent of project deletion and language choices. Public model requests do not
  use any AI-provider credential. No real model weights were downloaded in this work.
- Added an original vector logo: facing cream/blue book pages with speech blocks.
  The header places the logo before a smaller product name; SVG favicon and generated
  PNG/ICO/ICNS app icons share the same source. Regenerate with Tauri's icon command
  from public/logo.svg. No new design/runtime package dependency was added.
- Evidence: 304 Rust library tests, strict all-target Clippy, generated-contract check,
  frontend tests and production build passed. The nine new model tests use temporary
  caches and local HTTP servers. Browser fixture checks covered model progress/pause
  and the settings layout; the rendered icon was visually inspected. Native packaged
  installation and actual ONNX inference remain unverified; P09/P10 are not complete.

### Bounded translation batches

- Replaced the unrestricted remaining-chapters action with an explicit positive
  chapter count (default 10, remembered locally per project). Each run stops at
  the selected limit; starting the next batch remains a manual action.
- Admission requires maxChapters, applies it after excluding empty chapters and
  existing translations, and rejects empty work before constructing a provider.
  Glossary edits marking translations needs_review do not restart earlier chapters;
  replacing existing translations remains an explicit option. Job cancellation and
  resumption remain available in the job panel. A running translation disables a
  second start in the workspace.
- Regression coverage checks source order, skipped chapters, changed-glossary review
  status, explicit replacement, zero limits and empty selections. 305 Rust tests,
  strict Clippy, frontend tests and production build passed. No paid AI calls were run.
- Next parity stages: editable global book instructions, manual metadata and cover
  (including export), then the redesigned assistant using the new domain APIs.
  The current visual direction is retained per user feedback. Language choices stay
  fixed at project creation. These remaining stages are not yet implemented.

### Book details, cover and shared instructions

- Added revision-guarded human-owned title/author/annotation overrides and global
  translation instructions, separately from AI-generated metadata. New batch
  snapshots freeze the combined book/run instructions. Prompt changes advance the
  settings revision and mark existing ready translations for review without deleting them.
- Cover selection validates image content/size, stores immutable project assets,
  supports removal and survives reopen/archive transfer. EPUB source covers are
  preserved on import. Structured exporters use manual overrides and verify cover
  hashes before embedding. Additive format-1 table initialization supports existing
  new-format projects; no legacy data is revived or reset.
- The overview saves drafts before navigation/Ctrl+S/close, exposes generated-value
  fallback and protects stale writes. Browser fixture check verified editing the
  book prompt, switching to the reader and returning with the saved text intact.
- Evidence: 306 library tests passed; extended structural export tests passed with
  manual metadata and an embedded FB2 cover. Frontend tests/build passed. Clippy's
  redundant struct-update finding was removed before the final check. Native file
  chooser and packaged app acceptance remain unverified.

### P07 assistant through project services

- Restored a book-scoped assistant with persisted conversation history, current
  chapter/source/translation context, book instructions and a bounded glossary sample.
  Provider credentials stay outside the context/history; the assistant role resolves
  through the same profile mechanism as other services. No real provider request ran.
- Model replies can propose only book instructions, glossary upserts, literal
  replacement within the current chapter, or a bounded next-chapters translation.
  The server constructs project IDs/revisions and uses the same guarded application
  services as the UI. Unsupported actions/language changes are rejected.
- Added before/after previews, explicit apply/reject, request cancellation and opt-in
  automatic application for newly proposed actions. Pending previews expire after
  30 minutes/restart; history persists. Confirmations are single-use and project-scoped.
  Declining performs no domain or transcript write. Late source/translation/settings/
  glossary changes reject the response rather than silently using a stale context.
- Fake-provider regression covers cross-project rejection, denial, stale prompt
  proposals, successful application, duplicate application and invalid actions.
  Browser fixture verified chat → proposed prompt → apply → overview with saved text.
  Assistant tools are not advertised for the unimplemented manga processing pipeline.
- A concurrent regression exposed queued Tokio writes surviving an HTTP failure in
  the model downloader. Fixed separately in d29e438; all nine downloader tests passed.
- P07 is in progress: broader tool/context coverage, native acceptance and actual
  provider response-quality testing remain. This does not complete the manga phases.

### Batch glossary prerequisites

- Translation batches now optionally extract terms from exactly their selected
  chapters before translating any of them (enabled by default). Existing glossary
  targets/pins are preserved by the shared extractor. The assistant's bounded batch
  action uses the same prerequisite flow.
- The durable executor supports domain-defined step ordering and transactional
  dependency checkpoints. Only glossary revisions produced by the job are adopted;
  outside edits still conflict. All extraction completes before translation, so
  later extraction cannot invalidate completed translation steps during resume.
- Extended the structural fixture test with glossary creation and an injected context
  failure. Resume skips all completed extraction and translation work, keeps images,
  and completes contexts. 307 Rust tests, strict Clippy, frontend tests/build passed.

### P08 folder import and page navigation

- Added manga image-folder import through the creation wizard, sharing immutable
  asset/page publication with CBZ. Nested folders become naturally ordered volumes;
  traversal rejects symlinks and bounds depth/entry count. Folder names containing
  dots are correctly recorded as directory sources.
- The page list now renders a bounded visible window, follows direct page selection,
  and supports previous/next/number navigation. The main viewport supports fit-width,
  fixed zoom and pointer panning. Originals remain read-only.
- Eight project tests passed, including natural folder order (2 before 10), availability
  after deleting the synthetic source, and symlink rejection. Strict Clippy passed.
  Browser fixture with 1000 pages rendered eight thumbnail rows; direct jump to page
  1000 and zoom to 200% were verified. The synthetic SVG fixture is development-only.
- Dedicated raster thumbnails, EXIF normalization, RAR ingestion and measured native
  memory budgets remain P08 work. OCR/cleanup/lettering are still unavailable; this
  milestone does not claim automatic manga translation.

### P04 provider profile UI

- Added typed local profile list/save services with atomic revision-checked writes.
  Names, endpoints, models and generation options are editable; project translation
  and assistant roles can select separate profiles without changing languages.
- Credentials are profile-scoped and never returned by profile listing. Changing an
  endpoint clears its old credential unless a replacement is explicitly supplied.
  Generic legacy setting IPC now refuses profile writes and credential reads/writes.
- Profile persistence test covers secret exclusion, stale writes and endpoint changes.
  Strict Clippy passed. Browser fixture verified creating a named profile and assigning
  it only to the assistant role. No credential or network request was used in QA.
  Provider streaming/native response quality acceptance remains open.

### P08 canonical orientation and thumbnails

- Manga import decodes one page at a time with explicit dimension/output-allocation
  limits. EXIF rotation is applied once and normalized pixels are stored losslessly;
  unrotated originals retain their exact bytes. External source files are untouched.
- Every newly imported page references a registered PNG preview no larger than
  200×240. The virtual list uses previews; existing version-1 pages without previews
  remain readable. Preview assets participate in archive snapshots and FK validation.
- Added a real JPEG/EXIF orientation regression: a 400×200 image rotates to 200×400,
  creates a 120×240 preview and does not rotate again on reimport. Synthetic corrupt
  data is rejected. The first full run passed 309/310 tests; the remaining test used
  an outdated hand-built schema fixture and was switched to the shared current fixture.
  Native process memory/latency budgets remain unmeasured; decoder limits are not a
  substitute for those platform acceptance measurements.

### P05 book-wide search

- Added bounded, cursor-paginated literal search across source text or the latest
  chapter translations. Unicode queries and optional case sensitivity are supported.
- Search results open the matching chapter and scroll to the highlighted block.
  Browser fixture verified a translated-text search and navigation to chapter 3.
- All 311 Rust tests, frontend tests/build, generated contract check and strict Clippy
  passed. Tests cover Unicode, literal metacharacters, pagination and translation scope.
- These checks also close the outdated schema-fixture failure noted in the preceding
  thumbnail milestone; native performance and automatic manga acceptance remain open.

### P05 FB2 cover import and P13 workflow documentation

- FB2 loading now resolves the image referenced by `coverpage`, rather than treating
  the first binary illustration as a cover. Base64 data is bounded and decoded before
  the shared cover publication path validates and registers the image.
- Added regressions for the declared reference, missing references, malformed base64
  and propagation through the format-agnostic book loader. Inline FB2 illustrations
  remain a separate, unfinished fidelity task.
- Replaced obsolete README workflow claims (mutable languages, unlimited runs and
  legacy reference/retarget UI) with the current project, batch and assistant flow.
  Automatic manga processing and native acceptance are explicitly marked incomplete.
- Validation: all 52 book-module tests and strict all-target Clippy passed.

### P05 structural FB2 import

- Added structural FB2 parsing with embedded binary registration. Repeated image
  occurrences retain their positions while the asset store deduplicates bytes.
  Unnumbered and image-only sections are retained; nested content stays in reading
  order. External/missing image references fail explicitly instead of silently losing
  illustrations. Per-image and aggregate decoded-byte limits bound image imports.
- Removed the superseded numbered-section converter that discarded front matter.
- Export/import/export regression verifies three image occurrences, one image binary
  and identical structural blocks. Parser regression covers unnumbered/nested sections,
  mixed inline text, CDATA, repeated images and invalid references.
- Full suite passed 314 tests before deleting the obsolete converter and its test;
  strict all-target Clippy passed after that deletion. No manga feature changes.

### P05/P06 glossary review between batches

- Added literal case-sensitive search over source and target terms, a pinned-only
  filter and a matching-term count. Filtering and counting happen in SQLite before
  bounded pagination; editing continues to use revision guards.
- Regression covers pagination/counts, searching translated Unicode text, literal
  percent signs, pinned filtering and stale writes. Frontend tests/build, generated
  contracts and strict all-target Clippy passed.
- Browser fixture verified searching `остров` returns `the island`, then enabling
  pinned-only returns zero matches. Filtered empty results have their own message.

### P05 manual corrections after glossary/prompt changes

- Fixed literal replacement and reader edits rejecting retained translations after
  glossary, settings or chapter-instruction revisions changed. Manual corrections
  capture current inputs and preserve `needs_review` rather than claiming that a
  local edit revalidated the entire chapter.
- Preview publication still rejects any input/translation changes after preparation;
  reader saves still reject an obsolete translation version. Multi-chapter replacement
  remains atomic. Added regressions for review retention and stale-write rejection.
- Strict all-target Clippy passed; full library test results recorded below.
- Full library suite passed: 315 tests.

### P05 reference-aware glossary extraction

- Glossary extraction now includes the mapped reference excerpt, shared book
  instructions and relevant existing terms (pinned first). Reference text is limited
  to 16,000 characters and existing-term context to 16 KiB / 100 terms; source-term
  validation still checks only the original source chunk.
- Existing glossary targets remain authoritative during publication. A new extraction
  fingerprint version avoids reusing outputs from the earlier source-only prompt.
- Six book application tests passed, including a fake-provider batch/resume test that
  verifies reference/instruction context, truncation and preservation of a pinned target.
  Strict Clippy passed. Real-provider translation quality remains unmeasured.

### P06 reference mapping for long books

- Replaced one full reference dropdown per source chapter with two searchable,
  paginated lists (50 rows each). Mapping lookup is indexed; choosing a source and
  reference updates the draft, and the selected reference excerpt is visible.
- Added explicit unmapping and retained save/navigation-flush revision checks.
  Importing a replacement reference first flushes pending mapping edits.
- Browser fixture with 20,000 reference chapters rendered only 50 target rows;
  search for chapter 20,000, mapping and save were verified. Frontend tests/build
  passed. This bounds rendered rows; reference loading still transfers all reference
  text and needs a separate backend pagination/detail pass for large real books.

### P06 reference list payload

- Reference lists now return titles/IDs/mappings only; selecting a mapping fetches
  a Unicode-safe excerpt of at most 1,500 characters. Full text remains in SQLite
  and remains included in the streamed content fingerprint.
- Indexed structural-block lookup removes quadratic scans during book/reference
  import. Full library suite passed 316 tests; Clippy, frontend tests/build and
  generated contracts passed. Browser fixture verified the chapter-20,000 excerpt.
- The excerpt UI/API described above was rejected by the user and has now been
  removed. Reference adoption, rather than preview, is the required behavior.

### P05 restore reference adoption and legacy flags

- Reference import now copies whole translated chapters automatically into chapters
  without translations, matching chapter numbers and falling back to reading order.
  Source chapter numbers are retained on import; existing chapter titles provide a
  compatibility fallback. Existing translations are preserved. No provider call is
  made to adopt a reference. The complete body is stored intact in the first text
  block, without fabricating correspondence between source/reference paragraphs.
- Restored reader flags: origin `model` / `reference` / `manual`, chapter status
  `pending` / `in_progress` / `done` / `failed` / `skipped`, and `lang_issues` using
  the existing foreign-fragment scanner (including language-code aliases).
  Provider/step errors are exposed separately from detected foreign-language text.
  Manual body corrections change origin to `manual` and rescan language issues.
- Reference translations survive model-setting, glossary and prompt changes, remain
  exportable, and are skipped by ordinary bounded batches. Explicit forced model
  translation still replaces them and reports model origin.
- The excerpt command, DTOs, client method, selection effect and preview UI are gone;
  metadata-only reference lists remain bounded in rendered rows.

### P05 restore cumulative translation context

- Translation requests now explicitly include the saved rolling summary and the last
  1,200 Unicode characters of the previous text chapter's latest translation.
  Image-only chapters do not break continuity. The tail is read from actual saved
  text, including reference/manual translations, even if summary generation failed.
- Summary requests now combine the previous rolling summary with the newly translated
  chapter; the prior implementation summarized each chapter independently. Empty
  reference summaries retain the latest available earlier rolling summary.
- Fake-provider regression checks the actual translation/summary request payloads and
  resume after a context-step failure without translating completed chapters again.
  Reader failure status and structured error are checked before resume.
- Validation: full library suite passed 319 tests; the additional reference-tail
  continuity regression passed separately. Strict all-target Clippy, frontend tests,
  production build and generated-contract check passed. No paid-provider quality
  assessment or native desktop acceptance is implied by these deterministic tests.

### P05 manual corrections retain the restored flag semantics

- Bulk replacement's existing `manual-replace` provenance now maps to the legacy
  `manual` origin, just like direct editor corrections. Correcting a ready reference
  does not require model-input review merely because prompt/glossary settings changed.
- Final full library suite: 321 passed. Strict all-target Clippy passed. Frontend
  tests/build and contract checks passed earlier in this change; subsequent changes
  only affect backend correction semantics and regression coverage.

### P05 restore bounded language repair

- Ported the legacy maximum-two-pass, affected-line repair behavior to the new
  provider and structural translation pipeline. A title-only problem sends only
  the title. Every repair request includes the current project glossary.
- Repair runs before translation/context publication. Correct blocks, blank lines,
  indentation and image positions remain unchanged. Responses with duplicate or
  unsolicited line IDs, empty/shortened content or new line breaks are rejected.
  A correction must reduce detected foreign-language fragments before replacing text.
- Network failures or exhausted repairs retain the successful translation; remaining
  language issues are still visible through the restored reader flags. Importing a
  reference and manual saves do not invoke automatic model repair.
- Tests cover title-only requests with glossary, line/block preservation, malformed
  responses, two-pass limits, no-op/unsupported languages and network failure.
- Validation: full library suite passed 326 tests; strict all-target Clippy passed.
  This slice changes no frontend or IPC contract. Real-provider translation quality
  remains a separate acceptance check.

### P05 distinguish failed requests from language repair

- A wholly unusable translation response resends the unresolved original request,
  with the same glossary/instructions/rolling context, up to three total attempts.
  Empty text, malformed segment JSON, provider response-envelope errors and unknown
  segment IDs now follow the same bounded path. Partial successes remain retained:
  only missing/invalid segments are sent again.
- Network/429/5xx retries remain owned by the shared provider transport; permanent
  provider/configuration errors are not multiplied by a second retry loop.
  This is separate from the two-pass language repair of already translated lines.
- Eight book pipeline tests passed, including identical full retry payloads,
  malformed provider envelopes, bounded failures and partial-result preservation.
- Strict all-target Clippy passed. No frontend or contract changes in this slice.

### P05/P06 restore chapter-title actions

- Restored editable translated titles with serialized autosave and title-only model
  jobs. Body text, origin flags, review state and saved continuity survive title edits.
- Model title jobs send the source title, glossary and book/chapter instructions,
  never the chapter body. Durable completion survives interruption without repeating
  the paid step. Late model responses cannot overwrite newer manual corrections.
- Full library suite passed 330 tests; frontend tests and production build passed.
  Added mixed title/body autosave race and title conflict tests. The user's subsequent
  glossary-scope correction is the next slice: select only terms occurring in sent text.

### P05 restrict glossary payloads to text occurrences

- Chapter/title translation now selects only terms whose source occurs literally in
  the request's pending source segments. Selection is repeated after partial success,
  so retries do not resend terms from already accepted segments. Pinned terms are
  authoritative when matched, not an exception that sends unrelated entries.
- Language-repair requests select terms occurring in the affected lines, checking
  both source and translated forms. Prompt entries contain source/target/kind/pinned
  only; storage IDs, revisions and counters are not sent to the model.
- Glossary extraction already filters existing entries to each source chunk and
  retains that behavior. Matching remains case-sensitive/literal, as in the old pipeline.
- Full library suite passed 332 tests; strict all-target Clippy passed. Added actual
  request assertions for title-only exclusions, repair filtering and partial retries.

### P06 chapter-list status and origin filters

- Chapter summaries now expose the legacy chapter status, translation origin and
  review marker without loading source/translation bodies. A derived SQL view and
  indexed step lookup keep the list and reader on the same status calculation;
  there is no separately persisted mutable chapter-status copy.
- The virtualized chapter list displays status/origin/review and supports filters
  for pending, failed, done, review and reference chapters, combined with title search.
  It displays matching/total counts and retains bounded rendered rows.
- Job-state transitions refresh paginated metadata with a debounce. Reader saves
  update the current row immediately. List refreshes preserve the open editor and
  reject stale responses after local changes or project switches.
- Full library suite passed 333 tests; strict all-target Clippy passed. Frontend
  state tests cover refresh races and preservation of the active chapter object.
  Language-fragment details remain in the reader; this slice does not add a
  book-wide language-issue index/filter or claim native desktop acceptance.
- Frontend tests/build, generated-contract check and diff whitespace checks passed.

### P06 job remaining-time estimate

- Job snapshots expose remaining seconds for queued/running jobs, using per-stage
  averages from the current run or the latest 30 successful samples with the same
  job kind, processing settings and provider. Unknown stages show an estimating
  state; failed/cancelled/interrupted jobs do not display a running ETA.
- Counts use the latest attempt for each unit of work. Paused wall time is excluded.
  Estimates refresh at step boundaries; they are approximate, not a countdown and
  do not predict chapter length or future provider throttling.
- Full Rust suite: 334 passed. Strict all-target Clippy, frontend tests/build and
  generated-contract check passed. Added coverage for unknown samples, stage
  weighting, current-run preference, superseded attempts and changed settings.
