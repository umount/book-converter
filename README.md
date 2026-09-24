# book-converter

Desktop workspace for translating long books in controlled batches, with a shared
book prompt, glossary and reviewable AI assistant actions. Built with Rust, Tauri v2
and React/TypeScript.

The application is undergoing a staged refactor. The implementation and acceptance
record is in [REFACTORING_STATUS](docs/REFACTORING_STATUS.md); the remaining work is
tracked in [REFACTORING](docs/REFACTORING.md).

## Current workflow

1. Create a book or manga project and choose its source and target languages.
   The language pair stays fixed for the lifetime of the project.
2. Configure an API provider profile in Settings and assign translation and assistant
   roles. Profiles have independent endpoints, models and credentials.
3. For books, set the title, author, summary, cover and shared translation prompt
   in Overview. Imported EPUB and FB2 covers are retained when declared by the source.
4. Start a batch with an explicit chapter count. Optional glossary extraction runs
   before translation; processing stops after the selected batch. Review the glossary
   and results, adjust the prompt if needed, then start another batch.
5. Read original and translated blocks side by side, edit translations, search across
   either text and jump to a matching block. Existing results remain available when
   settings change and can be marked for review.
6. Use the assistant to discuss the current chapter and propose prompt changes,
   glossary entries, text replacements or bounded translation batches. Proposals show
   their changes before application; automatic application is an explicit opt-in.
7. Save a portable `.bcproj` archive or export the book as TXT, FB2, EPUB or PDF.

Book import supports TXT, FB2, EPUB, PDF and supported books inside ZIP archives.
Format fidelity is still being improved; FB2 inline illustrations are not yet carried
through the new importer.

Manga currently supports CBZ and image-folder import, natural page ordering,
orientation normalization, stored thumbnails, a virtualized page list, zoom and
panning. Local model assets are downloaded on demand. **Automatic manga recognition,
cleanup and lettering are not yet complete.** Extracted image folders can be used
for archives that are not supported directly. See [MANGA_TOOLING](docs/MANGA_TOOLING.md)
for model, licensing and native acceptance requirements.

## Development

Requirements: stable Rust/Cargo, Node.js/npm and the native Tauri prerequisites.
On Linux:

```bash
make deps-linux
make install
make dev
```

PDF support uses pdfium, fetched by `make dev` / `make binary` or `make fetch-pdfium`.
Provider profiles are configured in the app. Legacy environment configuration is
still present in the codebase; project language choices are not runtime settings.

Useful commands:

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo run --manifest-path src-tauri/Cargo.toml --example export_contracts -- --check
make binary
make bundle
```

For isolated Rust tests, set `XDG_DATA_HOME` to a temporary directory. Downloader
integration tests bind a local HTTP server. Browser fixtures exercise the interface
without provider requests; they do not validate native dialogs, OCR quality or
cross-platform packaging.

The release version is declared in `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
and `package.json`; `make version V=x.y.z` updates these together.

## Documentation

- [Refactoring plan](docs/REFACTORING.md)
- [Implementation and verification status](docs/REFACTORING_STATUS.md)
- [Manga tooling requirements](docs/MANGA_TOOLING.md)
- [Documentation index](docs/README.md)

Some older architecture and settings documents describe the previous implementation.
Consult the refactoring status before relying on their workflow or IPC descriptions.

## License

MIT.
