# book-converter — Architecture

> Last updated 2026-07-22 — synced with the implemented codebase.

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
| **config** | Configuration: DeepSeek API key (env `DEEPSEEK_API_KEY`), base_url, model, languages, `max_chunk_chars`, `max_retries` |
| **paths** | Shared app data directory (`XDG_DATA_HOME` / `~/.local/share/book-converter`) |
| **session** | Per-project `Session` + `AppState` map; project dirs; legacy cleanup |
| **jobs** | Background OS-thread runners for translation and glossary retarget |
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
| **translator::prompt** | System/user prompts: book identity + glossary + rolling summary + style; strict target-language rules; repair prompt |
| **translator::deepseek** | DeepSeek HTTP client, retry + backoff, continue on output truncation |
| **orchestrator** | Sequential translation loop (glossary + summary + enrich + target-language check) |
| **state** | Persist progress and glossary in per-project SQLite |
| **settings** | App-wide key-value settings DB |
| **export::txt** / **fb2** / **epub** / **pdf** | Assemble output formats |
| **i18n** | Output-facing localization from `assets/locales.json` |
| **commands/** | Tauri IPC — thin bridge; progress via events (`emit`) |

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
   │  4. deepseek.translate() ── retry ──>
   │  5. target-language check → one repair pass if foreign words remain
   │  6. save_translation (join chunks if split)
   │  7. update running_summary
   │  8. glossary extract + merge
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

1. **Parse** the reference into chapters.
2. **Align** by reading order / chapter numbers; seed pending chapters in continue mode.
3. **Bootstrap** pinned glossary from a ~30-chapter sample.
4. **Style exemplar** injected into translation prompts.

## State Storage (SQLite)

App data layout (see also `PROJECT_ISOLATION.md`):

```
<app_data_dir>/
  settings.db
  projects/<id>/
    progress.db
    project.json
```

| Table | Purpose |
|-------|---------|
| **chapters** | Source, status (`pending` / `in_progress` / `done` / `failed`), translation, origin |
| **glossary** | Canonical terms (`source`, `target`, `kind`, `frequency`, `pinned`) |
| **meta** | Title/author/cover/summary, `running_summary`, format, encoding |

## Error Handling and Limits

- **Retry:** network errors, `429`, `5xx` → exponential backoff, up to `max_retries`.
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
| `get_effective_config` | Resolved non-secret config for the Settings page (env-locked keys, key presence) |
| `get_app_info` | Product name, version, git commit/date, Tauri and platform (About dialog) |

### Project I/O
| Command | Purpose |
|---------|---------|
| `load_source` | Parse book into a new project DB |
| `open_project` | Restore project from DB alone |
| `delete_project` | Remove project data + session |
| `export_project` / `import_project` | `.bcproj` archive (manifest + DB) |

### Reference
| Command | Purpose |
|---------|---------|
| `load_reference` | Load reference; seed pending chapters |
| `bootstrap_glossary` | Pinned glossary from aligned sample |
| `harvest_glossary` | Extract terms from done chapters (start or end) into glossary |
| `use_reference_as_base` | Explicit continue-mode re-seed |

### Translation job
| Command | Purpose |
|---------|---------|
| `start_translation` / `pause_translation` | Start/resume or pause after current chapter |
| `get_progress` | Snapshot (also pushed via `progress` events) |
| `reset_translation` | Reset done → pending from a book chapter number |

### Glossary
| Command | Purpose |
|---------|---------|
| `get_glossary` / `update_term` / `delete_term` | Glossary CRUD |
| `retarget_terms` | Propagate renames (background; `retarget_*` events) |

### Reader / metadata
| Command | Purpose |
|---------|---------|
| `list_chapters` / `get_chapter` | Reader |
| `replace_in_book` | Find/replace across all stored translations (literal or regex) |
| `search_book` | Book-wide search, grouped per chapter (sidebar search panel) |
| `get_book_details` | Cover, summary, translated title/author |
| `translate_title` / `set_summary` / `generate_summary` / `set_cover` | Metadata |

### Export
| Command | Purpose |
|---------|---------|
| `export_book` | Export to path; format from extension |

Projects are isolated: `AppState` holds one `Session` per `project_id`. Events carry
`project`. See `docs/PROJECT_ISOLATION.md`.

## Deliberately Out of Scope (current)

- EPUB/HTML **input**.
- Parallel chapter translation (would drop rolling-summary continuity).
- Cloud sync, multi-user mode.
- Global rate limiter across parallel projects (rely on DeepSeek 429 handling).

The language pair and output format are configurable; the primary test run is
Chinese → Russian.
