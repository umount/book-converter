# book-converter — Architecture

> Last updated 2026-09-21, synced with the implemented codebase.

## Overview

book-converter is a desktop app that translates **large** books between languages
via the **DeepSeek API**. It is a **universal** converter, not tuned to one book:
the language pair is configurable, input and output come in multiple formats
(TXT, FB2, PDF, ZIP in; TXT, FB2, EPUB, PDF out), and anything book-specific (how
chapters are delimited, how names should be rendered) is inferred generically or
handed to the model — never hardcoded for a single title.

The primary test case is the web novel `光阴之外` / "За гранью времени"
(author 耳根 / Er Gen): a ~15 MB, **1354-chapter** Chinese book translated to
Russian.

The hard part is not the UI — it is the **translation pipeline**: a book cannot be
sent to the model in one shot, translation runs as hundreds of requests, the job
takes a long time, and it must survive restarts. A separate challenge is
**consistency**: the protagonist's name, locations, and setting terminology must be
translated identically across all chapters — optionally bootstrapped from a
professional **reference translation** when one is supplied.

## Core Principles

- **Universal, not book-specific.** Language pair is config; formats are pluggable;
  chapter detection tries generic patterns and, when they fail, asks the model to
  infer the delimiter. No logic is hardcoded to one title.
- **Deterministic where reliable, model where fuzzy.** Format parsing (XML
  structure, encoding, obvious chapter markers) is plain code. Anything semantic —
  detecting an unknown chapter delimiter, aligning a reference to the source,
  extracting names/lore, matching style — goes through DeepSeek.
- **Chunk by chapter, not by bytes.** A chapter is the natural unit and fits in one
  request (`deepseek-chat` context is 64K tokens). Character-based splitting is only
  a fallback for abnormally long chapters (`max_chunk_chars`), and it splits on
  paragraph boundaries.
- **Sequential, with rolling context.** Chapters are translated in order so each
  one carries the meaning of what came before. A compact **running summary** of the
  story so far (plus the previous chapter's closing lines) is fed into every
  chapter's prompt. Parallel chapter translation is **not implemented** (would drop
  cross-chapter context); see `DECISIONS.md`.
- **Persistent progress.** Every chapter's status and its translation live in SQLite
  under the per-project data directory. A run can be interrupted and resumed (only
  `pending`/`failed` chapters are translated). Idempotency is keyed on
  `chapter.index`.
- **The glossary is the source of consistency.** The canonical translation of
  names/locations/terms is fixed once and enforced in every later chapter.
- **UI-agnostic core.** All logic (parsing, chunking, the DeepSeek client, state,
  glossary, export) lives in a Rust library. Tauri/React call it through a thin
  command layer. The frontend can be replaced without touching the core.
- **Network fault tolerance.** Every request uses retry with exponential backoff
  (429/5xx/timeouts). One failed chunk does not sink the whole run.

## Stack

```
┌──────────────────────────────────────────────────────┐
│                      Tauri (v2)                        │
│              desktop shell + IPC bridge                │
├────────────────────────┬─────────────────────────────┤
│   Frontend (WebView)   │      Backend (Rust core)      │
│   React 18 + TS        │      book_converter_lib       │
│   Vite                 │      tokio (async runtime)    │
│                        │      reqwest → DeepSeek API    │
│                        │      rusqlite (SQLite)         │
│                        │      epub-builder / genpdf     │
│                        │      pdfium-render (PDF in)    │
├────────────────────────┴─────────────────────────────┤
│   Local: settings.db · projects/<id>/progress.db       │
│          TXT / FB2 / EPUB / PDF output                 │
└──────────────────────────────────────────────────────┘
                          │
                          ▼
                 DeepSeek API (external)
             https://api.deepseek.com/chat/completions
```

## Core Components

All modules live in `src-tauri/src/`. The command layer (`commands/`) is the only
thing the frontend knows about; session/job helpers sit beside it.

| Module | Responsibility |
|--------|----------------|
| **config** | Configuration: DeepSeek API key (settings DB, falling back to env `DEEPSEEK_API_KEY`), base_url, model, languages, `max_chunk_chars`, `max_retries` |
| **paths** | Shared app data directory (`XDG_DATA_HOME` / `~/.local/share/book-converter`) |
| **session** | Ephemeral per-project job state (`db_path`, cancel, running); validated project dirs / manifests |
| **jobs** | Shared background lifecycle (`jobs::lease` / `jobs::spawn`) + `jobs::translation` / `jobs::retarget` runners |
| **dto** | IPC response/request types shared by commands |
| **book::source** | Detect encoding (`chardetng`), decode (`encoding_rs`), zip unpack, PDF branch |
| **book::parser** | Chaptering via `assets/chapter_patterns.json`; model-inferred delimiter fallback |
| **book::pdf** | PDF text via bundled **pdfium** (Poppler / pure-Rust fallbacks); TOC; cover |
| **book::fb2** | Parse FB2 into sections → chapters |
| **book::load** | Format-agnostic input: detect TXT / FB2 / PDF, decode, parse → chapters + meta |
| **book::chunker** | Fallback splitting of an over-long chapter on paragraph boundaries |
| **reference** | Optional reference translation: load, align, bootstrap pinned glossary + style exemplar |
| **glossary** | Consistency: store terms, inject into prompt, auto-extract, merge with conflict resolution |
| **retarget** | Propagate a glossary rename into translated text + rolling context (inflection-aware) |
| **translator::prompt** | System/user prompts: book identity + glossary + rolling summary + style; strict target-language rules |
| **translator::reply** | The `<<<TITLE>>>` / `<<<BODY>>>` reply envelope, with the old first-line heuristic as fallback |
| **translator::repair** | Line-scoped language repair: which lines to send, the JSON prompt, and splicing replies back by line number |
| **translator::deepseek** | DeepSeek HTTP client, retry + backoff, continue on output truncation, `json_object` mode, OpenAI-style tool calling for the assistant |
| **assistant** | Project chat agent: compact snapshot + tool loop over `commands/ops` and Store, confirm gate, history in `assistant_messages` |
| **commands/ops** | Shared project operations (translation, glossary, search/replace, export) used by both IPC commands and the assistant |
| **orchestrator** | Sequential translation facade; delegates glossary learning and target-language repair to focused stages |
| **state** | Per-project SQLite: `chapters` / `glossary` / `meta` / `search` / `assistant` submodules over one `Store` |
| **settings** | App-wide key-value settings DB |
| **export::txt** / **fb2** / **epub** / **pdf** | Assemble output formats |
| **i18n** | Output-facing localization from `assets/locales.json` |
| **commands/** | Tauri IPC — thin bridge; reader metadata/search and project catalog/archive are separate submodules |

## Translation Pipeline

```
book.{txt,fb2,pdf,zip}
   │  book::load::load_book()
   ▼
[ Chapter{index, number, title, body} × N ]
   │  state::Store::init_chapters()
   ▼
SQLite projects/<id>/progress.db
   │  pending_chapters() in order
   ▼
SEQUENTIAL loop
   │  1. glossary::relevant_terms(body)
   │  2. running summary + prev chapter tail
   │  3. chunker if body > max_chunk_chars (paragraph boundaries)
   │  4. deepseek.translate() ── retry ──>  (reply framed: TITLE / BODY)
   │  5. target-language check → repair ONLY the lines that kept foreign
   │     words (title is line 0), spliced back by line number
   │  6. save_translation (join chunks if split)
   │  7. update running_summary
   │  8. glossary extract + merge + write the changed rows
   │  9. emit("progress", { project, … })
   ▼
export::{txt,fb2,epub,pdf}
```

## Glossary Subsystem

**Phase 1 — before translation.** Scan the chapter’s **original** text and inject
only glossary terms whose `source` appears there (`glossary::relevant_terms`) —
not the entire glossary — as a mandatory dictionary.

**Phase 2 — after translation.** Extract new terms; on conflict the fixed canon
wins. `pinned=true` terms are never overwritten by auto-extraction.

Categories (`TermKind`): `Person` · `Location` · `Organization` · `Term`.

## Input & Output Formats

| Format | Input | Output | Notes |
|--------|:-----:|:------:|-------|
| **TXT** | ✅ | ✅ | Heading patterns; model-inferred delimiter fallback |
| **FB2** | ✅ | ✅ | FictionBook XML; merge split parts (1.1, 1.2 …) |
| **EPUB** | — | ✅ | Per-chapter TOC via `epub-builder` |
| **PDF** | ✅ | ✅ | Input: pdfium (+ fallbacks); output: genpdf + lopdf outline |
| **ZIP** | ✅ | ✅ | Unpack on input; optional zip wrap on text formats |

Encoding on input is detected (UTF-8 / GBK / GB18030 / Big5, …).

## Reference Translation (optional)

Loading a reference is a **one-time import**. It parses the file, aligns it to the
source (by chapter number, falling back to reading order when either side is
unnumbered), and writes everything into the project database: the professional
chapters, the style exemplar, and the title/annotation/cover it contributes.

Afterwards nothing reads the reference file again:

| Step | Reads |
|---|---|
| Open the project (`get_reference_info`) | the database |
| Bootstrap the pinned glossary | the aligned pairs already in the database |
| Re-seed covered chapters (`use_reference_as_base`) | the database (a reset keeps the text, only status changes) |
| Style exemplar in prompts | `meta.ref_style` |

Activation used to call `load_reference`, which re-parsed the professional
translation **and** the whole source book on every open, for data that was
already stored. On a 1354-chapter book that was most of the wait.

## State Storage (SQLite)

App data layout (see also `PROJECT_ISOLATION.md`):

```
<app_data_dir>/
  settings.db
  projects/<id>/
    progress.db
    project.json
    assets/           images extracted from the source (`<content hash>.<ext>`)
```

| Table | Purpose |
|-------|---------|
| **chapters** | Source, status (`pending` / `in_progress` / `done` / `failed` / `skipped`), `kind` (`text` / `image` / `mixed`), translation, origin |
| **chapter_blocks** | Typed content of a chapter that is not plain prose: text, pictures and captions in page order |
| **assets** | Images extracted into `assets/`, by content hash (`rel_path`, MIME type, pixel size) |
| **glossary** | Canonical terms (`source`, `target`, `kind`, `frequency`, `pinned`) |
| **assistant_messages** | Per-project assistant transcript (`turn`, `role`, `content`, `tool_calls`) |
| **meta** | Sole durable source for title/author/cover/summary, `running_summary`, `book_prompt`, format, encoding |

## Chapters That Are Not Text

An EPUB page can be a whole picture — manga, illustrated editions, comic-style
web novels all ship spine items whose content is one `<img>` or `<svg><image/>`.
Such a page is read into typed **blocks**, and `chapters.source` stays the single
text projection of them, with each picture held in place by an `[[img:<id>]]`
line. That keeps one text pipeline: chunker, glossary, search, find/replace and
the reader's autosave all still work on a string.

The consequences of that one convention:

- Images are copied into `projects/<id>/assets/` at import and served to the
  webview over the `bookasset://` scheme (`assets`), never as data URLs.
- A page with no words is `skipped`: out of the queue, out of the ETA, and out of
  the progress denominator, so a manga volume can reach 100%.
- The model is told to copy marker lines through, and
  `book::restore_markers` puts back any it dropped, so a picture cannot be lost
  to a translation.
- EPUB, FB2 and PDF export re-embed the pictures; plain text drops the markers.

Every connection runs in WAL with a busy timeout: a translation run holds a
writer on a background thread while the UI opens short-lived readers, which the
default rollback journal would block outright.

Opening the database never changes it. Crash recovery (`in_progress` → `pending`)
is an explicit `Store::recover()`, called on project activation and at job start,
because reads happen constantly and a read that rewrote statuses would erase the
state of a run in flight.

## Error Handling and Limits

- **Retry:** network errors, `429`, `5xx` → exponential backoff, up to `max_retries`.
- **Repair scope:** a translation that keeps foreign words costs only the lines
  that contain them, not the chapter. A mangled repair loses its own line, never
  the chapter (see `translator::repair`).
- **Order:** chapters always run **sequentially** so the running summary carries forward.
- **Failure isolation:** a chapter that fails after retries is marked `failed`; the run continues.
- **Long chapters:** split via `book::chunker` when over `max_chunk_chars`.

## External Interfaces

- **DeepSeek API** — OpenAI-compatible `POST /chat/completions`. Auth:
  `Authorization: Bearer $DEEPSEEK_API_KEY`.
- **Filesystem** — input `.txt` / `.fb2` / `.pdf` / `.zip`; output same family + `.epub`;
  portable projects as `.bcproj` (manifest + `progress.db`).

## Frontend Commands (Tauri IPC)

### App settings
| Command | Purpose |
|---------|---------|
| `get_setting` / `set_setting` | Durable app-wide KV (UI language, language pair, model/advanced) |
| `get_effective_config` | Resolved non-secret config for the Settings page (env-locked keys, masked key hint) |
| `set_api_key` | Store or clear the API key; the generic setter refuses this key |
| `get_app_info` | Product name, version, git commit/date, Tauri and platform (About dialog) |

### Project I/O
| Command | Purpose |
|---------|---------|
| `load_source` | Parse book into a new project DB |
| `open_project` | Restore project from DB alone (also runs crash recovery) |
| `list_projects` | Every project found on disk, for reconciling the UI's list |
| `delete_project` | Remove project data + session |
| `export_project` / `import_project` | `.bcproj` archive (manifest + DB) |

### Reference
| Command | Purpose |
|---------|---------|
| `load_reference` | One-time import: parse, align, and write chapters + style + metadata into the DB |
| `get_reference_info` | What the DB knows about the attached reference (no file access) |
| `bootstrap_glossary` | Pinned glossary from aligned sample |
| `harvest_glossary` | Extract terms from done chapters (start or end) into glossary |
| `use_reference_as_base` | Explicit continue-mode re-seed |

### Translation job
| Command | Purpose |
|---------|---------|
| `start_translation` / `pause_translation` | Start/resume or pause after current chapter |
| `translate_chapter` | Translate one chapter from the reader |
| `translate_chapter_title` | Translate only the chapter title, leaving the body |
| `update_chapter_translation` | Save a hand-edited translation (refused while that chapter is `in_progress`) |
| `set_chapter_prompt` / `set_chapter_context` | Per-chapter instruction and rolling context |
| `set_book_prompt` | Book-wide translation instruction (every chapter) |
| `get_progress` | Snapshot (also pushed via `progress` events) |
| `reset_translation` | Reset done → pending from a book chapter number |

### Glossary
| Command | Purpose |
|---------|---------|
| `get_glossary_page` | One filtered, ordered window of the glossary plus the match total |
| `chapter_terms` | Only the terms occurring in one chapter (reader highlighting) |
| `update_term` / `delete_term` | Glossary CRUD (single-row writes) |
| `retarget_terms` | Propagate renames (background; `retarget_*` events) |

### Reader / metadata
| Command | Purpose |
|---------|---------|
| `list_chapters` / `get_chapter` | Reader (rows carry status, origin and any leftover foreign words) |
| `replace_in_book` | Find/replace across all stored translations (literal or regex) |
| `search_book` | Book-wide search, grouped per chapter (sidebar search panel) |
| `get_book_details` | Cover, summary, translated title/author, book prompt |
| `translate_title` / `set_summary` / `set_book_prompt` / `generate_summary` / `set_cover` | Metadata |

### Export
| Command | Purpose |
|---------|---------|
| `export_book` | Export to path; format from extension |

### Assistant
| Command | Purpose |
|---------|---------|
| `assistant_history` / `assistant_clear` | Load or wipe the per-project chat transcript |
| `assistant_state` | Whether a turn is running and any pending confirm (restored on project switch) |
| `assistant_send` / `assistant_approve` / `assistant_cancel` | Run a turn, resolve a confirm, or stop |

Projects are isolated: `AppState` holds one `Session` per `project_id`. Events carry
`project`. A `Session` contains only ephemeral job state; durable project data is
read from SQLite/manifest. Project ids are validated before they enter filesystem
paths. See `docs/PROJECT_ISOLATION.md`.

## Deliberately Out of Scope (current)

- EPUB/HTML **input**.
- Parallel chapter translation (would drop rolling-summary continuity).
- Cloud sync, multi-user mode.
- Global rate limiter across parallel projects (rely on DeepSeek 429 handling).

The language pair and output format are configurable; the primary test run is
Chinese → Russian.
