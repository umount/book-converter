# Book Converter

Desktop application for translating books and manga with a shared glossary,
editable results and resumable background jobs. Built with Rust, Tauri 2 and
React/TypeScript. The interface supports English, Russian and Chinese.

## Books

Import TXT, FB2, EPUB, PDF, or supported books inside ZIP archives. The reader keeps
text and illustrations as separate, ordered blocks. EPUB/FB2 illustrations and
available covers are retained; PDF extraction depends on the source document.

Translate a selected batch of chapters, optionally extracting terminology first.
Book and chapter instructions, matching glossary terms and preceding story context
are included in translation requests. Review and edit translations, search and
replace text, import an aligned reference translation, or use the book assistant
for proposed changes. Title, author, annotation and cover are managed in Overview.

Export books as TXT, FB2 in a `.fb2.zip` archive, EPUB or PDF. Portable `.bcproj`
archives contain project data and assets so work can be continued elsewhere.

## Manga

Import CBZ/ZIP or an image folder. Browse volumes and naturally ordered pages with
thumbnails, zoom and panning. The processing pipeline performs cloud recognition
and translation, followed by local text masks, inpainting and lettering.

Processing requires configured API access, installed model weights and the native
runtime. Regions can be moved/resized and locally rebuilt using existing translated
text. Original images remain unchanged. Portable `.bcproj` export is available;
rendered manga export to CBZ/EPUB is not currently implemented. RAR/CBR must be
extracted to a folder before import.

See [Books](docs/BOOKS.md) and [Manga](docs/MANGA.md) for the workflows and limitations.

## Quick start for development

Install Node.js/npm, stable Rust/Cargo and the native dependencies for Tauri.
On Debian/Ubuntu:

```bash
make deps-linux
make install
make dev
```

`make dev` downloads pdfium if needed and starts the desktop application with Vite.
For local manga processing, also prepare the native resources:

```bash
npm run manga:prepare
```

Model weights are downloaded separately in **Settings → Manga models**. Configure
provider access in Settings, create a project, choose its source and target languages,
and start an explicit chapter/page batch. Opening a project does not start processing.

```bash
make binary   # Release application without an installer
make bundle   # Platform-specific installers
```

Release builds prepare the manga runtime automatically. The runtime directory must
remain available alongside an unbundled executable or in the application's resources.
See [Development](docs/DEVELOPMENT.md) for checks, packaging and test commands, and
[Settings](docs/SETTINGS.md) for providers and local data locations.

## Documentation

- [Architecture and interaction diagrams](docs/ARCHITECTURE.md)
- [Book workflow](docs/BOOKS.md)
- [Manga workflow](docs/MANGA.md)
- [Native manga runtime](docs/MANGA_RUNTIME.md)
- [Settings and provider profiles](docs/SETTINGS.md)
- [Book assistant](docs/ASSISTANT.md)
- [Development and verification](docs/DEVELOPMENT.md)
- [Documentation index](docs/README.md)

## License

Book Converter is free and open-source software under the [MIT License](LICENSE.md).
Third-party libraries, fonts and model weights retain their own licenses; bundled
notices are in [third-party](third-party/) and the prepared runtime resources.
