# Development and verification

## Language conventions

Write project documentation, commit messages and code comments in English.
Localized interface strings and multilingual test data retain their required
languages. Non-English examples in English-language explanations are appropriate
when they demonstrate parsing, translation or language-specific behavior.

## Local setup

Install stable Rust/Cargo, Node.js/npm and the native Tauri prerequisites for your
platform. The repository's Debian/Ubuntu helper installs the required system packages:

```bash
make deps-linux
make install
make dev
```

`make dev` fetches pdfium and starts Tauri with Vite on port 1420. `npm run dev` starts
only Vite; it does not provide native IPC. To work on UI fixtures without Tauri, open
`http://localhost:1420/?preview=1`. Fixtures are enabled only in development and make
no real provider requests. They do not validate native file dialogs or model quality.

Narration in development additionally requires Python 3.11/3.12 with pip and venv,
then `npm run tts:prepare`. The installed application includes a frozen speech
runtime. See [Narration](NARRATION.md) for model downloads, checks and CUDA builds.

## Source layout

- `src/app/`: application shell, settings, localized strings and Jobs UI.
- `src/features/`: book, glossary, assistant and project screens.
- `src/shared/`: typed API, generated contracts, state stores and shared UI.
- `src-tauri/src/app/`: Rust contracts and application composition.
- `src-tauri/src/commands/`: Tauri command adapters.
- `src-tauri/src/application/`: domain workflows and provider-backed processing.
- `src-tauri/src/project/`, `storage/`, `assets/`: lifecycle and persistence.
- `src-tauri/src/ai/`: external provider transport.
- `src-tauri/src/models/`, `narration/`, `scripts/tts/`: model downloads and offline MP3 generation.
- `tests/frontend/`, `tests/fixtures/`: frontend checks and deterministic input data.

The [architecture](ARCHITECTURE.md) explains dependency direction and data ownership.

## Routine checks

Run checks appropriate to the changed boundary:

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run contracts:check
```

`npm test` compiles the pure TypeScript API/state modules and runs Node tests.
Rust tests cover persistence, import/export, provider payloads, stale-result guards,
checkpoint recovery and domain workflows. Provider integration tests use local HTTP servers.
These commands are verification instructions, not a stored claim about a particular run.

Use a temporary `XDG_DATA_HOME` when testing code that accesses application-wide settings:

```bash
TEST_DATA_DIR=$(mktemp -d)
XDG_DATA_HOME="$TEST_DATA_DIR" cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Inspect/remove that temporary directory after use. Test providers and synthetic fixtures
should be used for routine checks; live-provider runs require configured access and
can incur provider charges.

## IPC changes

Rust DTOs are canonical. After changing them, regenerate and check TypeScript contracts:

```bash
npm run contracts:generate
npm run contracts:check
npm run build
```

Do not manually maintain a second DTO definition. Wire new commands through Rust
registration and the typed frontend transport, and preserve structured error fields.

Regenerate portable format fixtures with `python3 scripts/generate-fixtures.py`.
Their provenance and expected structure are documented in
[tests/fixtures/README.md](../tests/fixtures/README.md).

## Release builds

```bash
make binary
make bundle
make version V=x.y.z
make show-version
```

`make binary` produces `src-tauri/target/release/book-converter` on Linux;
`make bundle` builds platform installers. Both run the configured Tauri build hooks,
including the frontend build and `npm run tts:bundle` (Python 3.11/3.12 required on
the build machine). PDFium and the `tts-runtime/` directory must be included beside
the standalone binary; `make binary` copies both. Installers include these resources.

The version is declared in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`; the version helper updates these together. Vite embeds a
build timestamp/identifier for **Help → About**. Inspect generated manifests, bundled
resources and notices before distributing a target build.

## Resume a book job without the UI

The maintenance example uses the same durable pipeline and saved provider settings:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --example resume_book_job -- \
  /absolute/path/to/book-converter PROJECT_ID JOB_ID
```

This executes real processing, can send the selected source text to the configured
provider and writes results into that project. Do not run the same job concurrently
from the desktop application. The example does not emit webview updates; reopen the
project in the UI after completion. For ordinary use, prefer **Resume** in Jobs.


## Windows diagnostic installer for testers

On a Windows x64 build machine with Node.js, Python 3.11/3.12, Rust MSVC and Visual
Studio C++ build tools, run from PowerShell:

```powershell
npm run build:debug:windows
```

The script installs npm dependencies, fetches pdfium, and builds an NSIS installer
using `tauri build --debug --features diagnostics`.
Output: `src-tauri/target/x86_64-pc-windows-msvc/debug/bundle/nsis/*-setup.exe`.

Send the **installer**, not the bare application executable. Testers do not need
Rust, Node.js or Python.
The diagnostic feature defaults full logging to on unless the user has explicitly
saved a different preference. This build includes debug information and is larger
than a release build.

The manually dispatched **Windows diagnostic installer** GitHub Actions workflow
builds the same installer and uploads it as `book-converter-windows-debug`.
It does not publish a release or run automatically on pushes.

For a report: reproduce the issue, open **Settings → Diagnostics → Save diagnostic
logs**, and attach the ZIP. It includes build identification and recent logs, not
settings, provider credentials or project contents. Both regular and diagnostic
builds have the logging checkbox; it takes effect immediately.
