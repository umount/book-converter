# Refactoring execution status

## Current phase

P00: completed. Baseline and deterministic fixtures.
P01: completed. Typed contracts, composition boundary and frontend test harness.
P02: completed. Versioned repositories, immutable assets and domain result persistence.
P03: backend lifecycle implemented; reset execution and UI activation remain.
P04: in_progress. Durable execution and structured provider transport implemented.
P05: in_progress. Structural translation, context and manual saves implemented.
P06–P13: pending; typed frontend API methods prepared.

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

Continue P05 editing/reference/export and independent metadata/glossary jobs, then
P06 workspace integration. Close remaining P04 acceptance items before cutover.
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
