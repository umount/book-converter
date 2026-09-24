# Refactoring execution status

## Current phase

P00: completed. Baseline and deterministic fixtures.
P01: completed. Typed contracts, composition boundary and frontend test harness.
P02: completed. Versioned repositories, immutable assets and domain result persistence.
P03: lifecycle wired to the new UI; explicit legacy reset executed. Native acceptance remains.
P04: in_progress. Durable execution and structured provider transport implemented.
P05: in_progress. Translation, context, editing, reference, export and metadata backend implemented.
P06: in_progress. New shell, wizard, structural editor and supporting screens wired; native acceptance remains.
P07: pending.
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

Run the P06 Linux Tauri import → metadata → translation → edit → search → export
acceptance flow, including structural illustrations and native close/save handling.
Then continue provider-role UI/streaming acceptance and P07 application-service tools.
Languages are immutable after project creation; do not implement language retarget.
Close remaining P04/P05 acceptance items before declaring their phases complete.
Plan snapshot committed as a45d990. Baseline formatting drift is recorded, not hidden.

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
