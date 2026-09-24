# Refactoring execution status

## Current phase

P00: completed. Baseline and deterministic fixtures.
P01: completed. Typed contracts, composition boundary and frontend test harness.
P02: in_progress. Schema/asset prototype committed; repositories and protocol remain.
P03–P13: pending. No production behavior has been replaced yet.

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

Continue P02 repositories and asset protocol integration.
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
- This is deliberately not a completed P02; repositories and protocol integration
  need further work before project lifecycle replacement.

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
