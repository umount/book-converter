# book-converter

Desktop app for translating **long books** with an LLM while keeping names, lore,
and style consistent across the whole work — built for novels that do not fit in
a single model request.

**Stack:** Rust core (`book_converter_lib`) · Tauri v2 · React/TypeScript UI ·
**DeepSeek** API (`deepseek-chat`).

## What it does

| Area | Capabilities |
|---|---|
| **Input** | TXT, FB2, PDF, ZIP · encoding auto-detected (UTF-8 / GBK / GB18030 / Big5) · chapter patterns + model-inferred delimiter / PDF TOC fallback |
| **Translation** | Sequential by chapter · rolling story summary + previous-chapter tail · per-chapter context persisted in SQLite · pause / resume · optional limit (“next N”) · long chapters split on paragraph boundaries |
| **Consistency** | Auto-growing glossary (person / location / organization / term) · pin & edit in UI · propagate renames into existing text (retarget) |
| **Reference** | Load a professional translation · bootstrap pinned glossary + style exemplar · seed covered chapters and continue from where it ends |
| **Per chapter** | Translate one chapter from the reader · **custom chapter prompt** (e.g. “господин, not госпожа”) without touching the glossary · retranslate with that prompt · manual edit of title/body |
| **Projects** | Each book is an isolated project (own DB, glossary, progress, console) · parallel runs · portable `.bcproj` (manifest + DB) |
| **Output** | FB2, EPUB, PDF, TXT · optional zip for text formats · cover, title, author, annotation |
| **UI** | IDE-like: sidebar, overview, dual-pane reader (original ↔ translation + glossary highlight), glossary table, console, settings (UI + language pair) |

## Highlights

- **Book-length runs.** Progress lives in SQLite under `projects/<id>/`; interrupt
  and resume anytime. Rolling summary is stored per chapter so a single-chapter
  retranslate or a mid-book resume keeps narrative continuity.
- **Glossary first.** The full glossary can grow large, but each chapter’s prompt
  only gets terms whose **source form appears in that chapter’s original text**
  (not the whole dictionary). After translation, new terms are extracted and
  merged; pinned entries win on conflict.
- **Reference as canon, not copy-paste.** Mine names/style from a pro translation;
  optionally keep those chapters and machine-translate only what follows.
- **Chapter-level control.** Custom prompt for one chapter, one-shot translate /
  retranslate, or hand-edit the result — without polluting the global glossary.
- **Universal.** Language pair is a setting; formats and chapter detection are
  generic, not hardcoded to one title.

## Requirements

- **Rust** (stable) + Cargo
- **Node.js** ≥ 18 + npm
- **Linux system libraries** for Tauri:

  ```bash
  make deps-linux   # or:
  sudo apt install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

- A **DeepSeek API key**
- PDF engine: pdfium is fetched by `make dev` / `make binary` (or `make fetch-pdfium`)

## Setup

```bash
make install            # frontend + CLI deps
cp .env.example .env     # set DEEPSEEK_API_KEY=sk-...
```

The key is read from `.env` (auto-loaded) or `DEEPSEEK_API_KEY`.

## Run

```bash
make dev        # hot-reload window
make binary     # release binary → src-tauri/target/release/book-converter
make bundle     # .deb / .rpm / .AppImage
make run        # build release binary and launch
make check      # cargo check + tsc
make test       # Rust unit tests
```

## Using the app

### Projects

1. **File → Open book** — `.txt` / `.fb2` / `.pdf` / `.zip` → new project in the
   sidebar (same file opened twice = two projects).
2. **File → Save project** / **Open project** — `.bcproj` archive (manifest +
   progress DB; original book file not required after import).
3. Several projects can translate at once; each has its own progress and console.

### Overview

1. *(Optional)* **Open reference** → **Bootstrap** glossary from a sample of
   aligned chapters. Covered chapters can be seeded so you only translate the rest.
2. Set “next N chapters” or leave blank for all → **Start** / **Pause**.
3. **Retranslate** from chapter N (or whole book) with the current glossary, then
   Start again.
4. Cover, translated title/author, and annotation can be edited or generated.

### Translation (reader)

1. Side-by-side **original** and **translation**, with glossary highlighting.
2. Untranslated chapter → **Translate this chapter**.
3. **Prompt** chip → per-chapter instruction (not the glossary), e.g. how to render
   a character’s gender/title. **Save prompt** or **Retranslate with prompt**.
4. Done chapter → **Edit** title/body manually (marked as edited).

### Glossary

1. Browse / filter / add / pin terms; change a rendering to queue a rename.
2. **Update translation** rewrites affected paragraphs via the model (inflection-aware).

### Export

**File → Export as** FB2 / EPUB / PDF / TXT (zipped when useful). Uses stored
cover, titles, and summary.

## Documentation

| Document | Description |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Pipeline, modules, IPC |
| [`docs/DECISIONS.md`](docs/DECISIONS.md) | Design decisions |
| [`docs/PROJECT_ISOLATION.md`](docs/PROJECT_ISOLATION.md) | Per-project layout, parallel runs, `.bcproj` |
| [`docs/SETTINGS.md`](docs/SETTINGS.md) | Env / settings DB / localStorage / project meta |
| [`docs/README.md`](docs/README.md) | Docs index |

## Configuration

Environment / `.env`:

| Variable | Default | Purpose |
|---|---|---|
| `DEEPSEEK_API_KEY` | — | API key (required) |
| `DEEPSEEK_MODEL` | `deepseek-chat` | Model |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | API base URL |
| `SOURCE_LANG` | (UI / Chinese) | Override source language |
| `TARGET_LANG` | (UI / Russian) | Override target language |

UI language and the translation language pair are also set under **Settings** and
stored in the app settings DB. Full matrix: [`docs/SETTINGS.md`](docs/SETTINGS.md).

## License

MIT.
