# book-converter

A desktop tool for translating **large books** with an LLM while keeping names,
lore, and style consistent across the whole work — built for novels that are far
too long to translate in a single request.

Powered by the **DeepSeek API**. Rust core, Tauri v2 + React desktop app.

## Highlights

- **Handles book-length input.** A book is split into chapters and translated
  sequentially; progress is stored in SQLite, so a run can be paused, interrupted,
  and resumed at any time. Abnormally long chapters are split on paragraph
  boundaries automatically.
- **Consistency where it matters.** An auto-growing **glossary** fixes the
  canonical translation of names, places, sects, and terminology and enforces it
  in every chapter.
- **Narrative continuity.** Chapters are translated in order with a **rolling
  summary** of the story so far, so meaning is not lost between chapters.
- **Learn from a professional translation.** Point it at an existing reference
  translation and it bootstraps a **pinned glossary** (names/lore) and a **style
  exemplar**, and can **continue** the translation from where the reference ends.
- **Universal, not hardcoded.** Configurable language pair; chapter detection uses
  generic patterns and falls back to a model-inferred delimiter for unknown layouts.
- **Formats.** Input: TXT, FB2, PDF, ZIP (encoding auto-detected — UTF-8 / GBK /
  GB18030 / Big5). Output: FB2, EPUB, PDF, TXT — pick the format directly, output
  optionally zipped. Book title, cover, and annotation are carried over (and can
  be replaced).
- **Isolated projects.** Each open book is its own project (own DB, glossary,
  progress, console). Several books can translate in parallel. Save/open as a
  portable `.bcproj` archive.
- **IDE-style app.** Projects sidebar, chapter reader with side-by-side
  **original ↔ translation** panes and glossary highlighting, editable glossary.

## Requirements

- **Rust** (stable) + Cargo
- **Node.js** ≥ 18 + npm
- **Linux system libraries** for Tauri:

  ```bash
  make deps-linux   # or:
  sudo apt install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

- A **DeepSeek API key**.
- For PDF input/output, pdfium is fetched automatically by `make dev` /
  `make binary` (or run `make fetch-pdfium` once).

## Setup

```bash
make install            # frontend + CLI deps
cp .env.example .env     # then set DEEPSEEK_API_KEY=sk-...
```

The key is read from `.env` (auto-loaded) or the `DEEPSEEK_API_KEY` environment
variable.

## Run

```bash
make dev        # hot-reload dev window
make binary     # standalone release binary → src-tauri/target/release/book-converter
make bundle     # installers (.deb / .rpm / .AppImage)
make run        # build the release binary and launch it
make check      # cargo check + tsc
make test       # Rust unit tests
```

## Using the app

1. **Open book** — pick a `.txt` / `.fb2` / `.pdf` / `.zip`. It becomes a project
   in the sidebar.
2. *(Optional)* **Open reference** — a professional translation of the same book;
   then **Bootstrap** the glossary from it. Loading a reference also seeds covered
   chapters so you can **continue** from where the professional text ends.
3. Set how many chapters to translate (or leave blank for all) and press **Start**.
   Watch progress; **Pause** stops after the current chapter. You can work on
   another project while one is translating.
4. **Translation** view: read any chapter with original and translation side by
   side, with glossary terms highlighted.
5. **File → Export as** FB2 / EPUB / PDF / TXT. Cover, title, and summary are
   included.
6. **File → Save project** / **Open project** — portable `.bcproj` (manifest +
   progress DB; no copy of the original book needed).

## Documentation

| Document | Description |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Components, pipeline, glossary, IPC |
| [`docs/DECISIONS.md`](docs/DECISIONS.md) | Design decisions and rationale |
| [`docs/PROJECT_ISOLATION.md`](docs/PROJECT_ISOLATION.md) | Per-project data, parallel runs, `.bcproj` |
| [`docs/SETTINGS.md`](docs/SETTINGS.md) | Env / settings DB / localStorage / project meta |
| [`docs/README.md`](docs/README.md) | Docs index |

## Configuration

Environment / `.env`:

| Variable | Default | Purpose |
|---|---|---|
| `DEEPSEEK_API_KEY` | — | API key (required) |
| `DEEPSEEK_MODEL` | `deepseek-chat` | Model |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | API base URL |
| `SOURCE_LANG` | (UI setting / Chinese) | Override source language |
| `TARGET_LANG` | (UI setting / Russian) | Override target language |

UI language and the translation language pair are also set in **Settings** and
persisted in the app settings DB. See [`docs/SETTINGS.md`](docs/SETTINGS.md) for the
full matrix.

## License

MIT.
