# Book Converter

Desktop application for translating books with a glossary,
editable results and resumable background jobs. Built with Rust, Tauri 2 and
React/TypeScript. The interface supports English, Russian and Chinese.

## Books

Import TXT, FB2, EPUB, PDF, or supported books inside ZIP archives. The reader keeps
text and illustrations as separate, ordered blocks. EPUB/FB2 illustrations and
available covers are retained; PDF extraction depends on the source document.

Translate a selected batch of chapters. The glossary updates after each chapter and reference import.
Book and chapter instructions, matching glossary terms and preceding story context
are included in translation requests. Review and edit translations, search and
replace text, import an aligned reference translation, or use the book assistant
for proposed changes. Title, author, annotation and cover are managed in Overview.

Export books as TXT, FB2 in a `.fb2.zip` archive, EPUB or PDF. Portable `.bcproj`
archives contain project data and assets so work can be continued elsewhere.

See [Books](docs/BOOKS.md) for workflows and limitations.

Create audiobooks locally with Qwen3-TTS in the **Narration** tab. Choose the
original or translation, chapters and a preset voice; pause and resume generation,
then export chapter MP3s with a playlist. See [Narration](docs/NARRATION.md) for
model downloads, supported languages and CPU/CUDA setup.

## Quick start for development

Install Node.js/npm, stable Rust/Cargo and the native dependencies for Tauri.
On Debian/Ubuntu:

```bash
make deps-linux
make install
make dev
```

`make dev` downloads pdfium if needed and starts the desktop application with Vite.
Configure
provider access in Settings, create a project, choose its source and target languages,
and start an explicit chapter batch. Opening a project does not start processing.

```bash
make binary   # Release application without an installer
make bundle   # Platform-specific installers
```

See [Development](docs/DEVELOPMENT.md) for checks, packaging and test commands, and
[Settings](docs/SETTINGS.md) for providers and local data locations.

## Windows: development and builds

Windows x64 is supported. To build from source, install:

- Node.js 22 with npm.
- Python 3.11 or 3.12 with pip (build machine only, for the bundled speech engine).
- Rust through rustup with the `stable-x86_64-pc-windows-msvc` toolchain.
- Visual Studio 2022 Build Tools with **Desktop development with C++**, MSVC and the Windows SDK.
- Microsoft Edge WebView2 Runtime, if it is not already installed.

Open PowerShell in the project directory. These commands do not require `make`:

```powershell
npm ci
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-windows.ps1
npm run tauri -- dev
```

Preparation downloads PDFium. The Vite server alone does not provide native features.

### Release installer

After preparing the dependencies and pdfium, build the installer:

```powershell
npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis
```

The installer is written to
`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`.
Distribute the installer so PDFium and other resources are included.

### Diagnostic build for testers

```powershell
npm run build:debug:windows
```

The script runs `npm ci`, prepares pdfium and builds an installer with debug information and full diagnostic logging enabled by default.
Output: `src-tauri/target/x86_64-pc-windows-msvc/debug/bundle/nsis/*-setup.exe`.
Testers do not need Node.js, Rust or Python; they need the installer, configured API
access.

After reproducing an issue, select **Settings → Diagnostics → Save diagnostic logs**.
The **Full diagnostic logging** checkbox takes effect immediately and persists across
restarts. API keys are redacted. Rejected translation responses can include source or translated text in logs. A previously saved
preference to disable logging also applies to diagnostic builds.

The manual **Windows diagnostic installer** GitHub Actions workflow builds the same
installer and uploads it as the `book-converter-windows-debug` artifact.

## Documentation

- [Architecture and interaction diagrams](docs/ARCHITECTURE.md)
- [Book workflow](docs/BOOKS.md)
- [Local narration and MP3 export](docs/NARRATION.md)
- [Settings and provider profiles](docs/SETTINGS.md)
- [Book assistant](docs/ASSISTANT.md)
- [Development and verification](docs/DEVELOPMENT.md)
- [Documentation index](docs/README.md)

## License

Book Converter is free and open-source software under the [MIT License](LICENSE.md).
Third-party libraries and fonts retain their own licenses; bundled
notices are in [third-party](third-party/).
