# Development and verification

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

Prepare local manga resources separately during development:

```bash
npm run manga:prepare
```

Download weights through Settings. See [Native runtime](MANGA_RUNTIME.md) for resource
layout, supported build targets and model installation.

## Source layout

- `src/app/`: application shell, settings, localized strings and Jobs UI.
- `src/features/`: book, manga, glossary, assistant and project screens.
- `src/shared/`: typed API, generated contracts, state stores and shared UI.
- `src-tauri/src/app/`: Rust contracts and application composition.
- `src-tauri/src/commands/`: Tauri command adapters.
- `src-tauri/src/application/`: domain workflows and provider-backed processing.
- `src-tauri/src/project/`, `storage/`, `assets/`: lifecycle and persistence.
- `src-tauri/src/ai/`, `models/`: external provider transport and model downloads.
- `crates/manga-inference/`: local worker, ONNX adapters, text shaping and layout.
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
checkpoint recovery and domain workflows. Downloader integration tests bind local HTTP
servers. Native-model tests are explicitly ignored unless their resources are supplied.
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

## Native worker tests

Worker protocol/lettering tests can run without downloading model weights:

```bash
cargo test --manifest-path crates/manga-inference/Cargo.toml --features onnx,worker
```

The actual inference smoke test is opt-in. Set absolute paths to the prepared runtime
library and installed mask/LaMa artifacts, then run:

```bash
MANGA_RUNTIME=/absolute/path/to/runtime-library \
MANGA_MASK_MODEL=/absolute/path/to/model.safetensors \
MANGA_LAMA_MODEL=/absolute/path/to/weights.onnx \
cargo test --manifest-path crates/manga-inference/Cargo.toml \
  --features onnx,worker --test native_smoke -- --ignored
```

Application-level native pipeline tests additionally require `MANGA_WORKER` to point
to the prepared executable. Their requirements are stated on the ignored tests in
[application/manga/tests.rs](../src-tauri/src/application/manga/tests.rs).
Native GUI behavior, representative pages and each packaged target need their own
checks; a fixture screenshot or synthetic CPU test does not establish those results.

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
including native manga resource preparation and the frontend build. pdfium and
manga-runtime resources must be available in the deployed application layout.

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
