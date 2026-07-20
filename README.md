# book-converter

A desktop tool for translating **large books** with an LLM while keeping names,
lore, and style consistent across the whole work — built for novels that are far
too long to translate in a single request.

Powered by the **DeepSeek API**. Rust core, Tauri + React desktop app.

## Highlights

- **Handles book-length input.** A book is split into chapters and translated
  sequentially; progress is stored in SQLite, so a run can be paused, interrupted,
  and resumed at any time.
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
- **Formats.** Input: TXT, FB2, ZIP (encoding auto-detected — UTF-8 / GBK /
  GB18030 / Big5). Output: FB2, EPUB, TXT, optionally zipped. Book title, cover,
  and annotation are carried over (and can be replaced).
- **IDE-style app.** A projects sidebar (each book is a project), a chapter reader
  with side-by-side **original ↔ translation** panes and glossary highlighting, an
  editable glossary, and per-project state.

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
```

## Using the app

1. **Open book** — pick a `.txt` / `.fb2` / `.zip`. It becomes a project in the
   sidebar.
2. *(Optional)* **Open reference** — a professional translation of the same book;
   then **Bootstrap** the glossary from it. Enable **Continue mode** to keep the
   professional chapters and translate only what follows.
3. Set how many chapters to translate (or leave blank for all) and press **Start**.
   Watch progress; **Pause** stops after the current chapter.
4. **Translation** view: read any chapter with original and translation side by
   side, with glossary terms highlighted.
5. **Export** to FB2 / EPUB / TXT (zipped if you like). Cover, title, and summary
   are included.

## Documentation

See [`docs/`](docs/README.md): architecture and design decisions.

## Configuration

Environment / `.env`:

| Variable | Default | Purpose |
|---|---|---|
| `DEEPSEEK_API_KEY` | — | API key (required) |
| `DEEPSEEK_MODEL` | `deepseek-chat` | Model |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | API base URL |

## License

MIT.
