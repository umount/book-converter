# book-converter — Architecture

> Last updated 2026-07-17 — initial version, in sync with the code scaffold.

## Overview

book-converter is a desktop app that translates **large** books between languages
via the **DeepSeek API**. It is a **universal** converter, not tuned to one book:
the language pair is configurable, input and output come in multiple formats
(TXT, FB2; EPUB planned), and anything book-specific (how chapters are delimited,
how names should be rendered) is inferred generically or handed to the model —
never hardcoded for a single title.

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
  a fallback for abnormally long chapters, and it splits on paragraph boundaries.
- **Sequential, with rolling context.** Chapters are translated in order so each
  one carries the meaning of what came before. A compact **running summary** of the
  story so far (plus the previous chapter's closing lines) is fed into every
  chapter's prompt, so continuity — ongoing scenes, who is who, unresolved threads —
  is not lost between chapters. This favors coherence over raw speed (see
  `DECISIONS.md`); an optional parallel mode trades that continuity for throughput.
- **Persistent progress.** Every chapter's status and its translation live in SQLite.
  A run can be interrupted at any point and resumed (only `pending`/`failed` chapters
  are translated). Idempotency is keyed on `chapter.index`.
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
│                        │      epub-builder              │
├────────────────────────┴─────────────────────────────┤
│   Local: SQLite (progress.db) · TXT/EPUB output        │
└──────────────────────────────────────────────────────┘
                          │
                          ▼
                 DeepSeek API (external)
             https://api.deepseek.com/chat/completions
```

**Why Tauri + React instead of iced:** the UI here is data-heavy — an editable
glossary table, a list of 990 chapters with statuses, an "original ↔ translation"
preview. Web technologies make this simpler and better-looking while the core stays
in Rust. See `DECISIONS.md` § UI stack choice.

## Core Components

All modules live in `src-tauri/src/`. The command layer (`commands.rs`) is the only
thing the frontend knows about.

| Module | Responsibility |
|--------|----------------|
| **config** | Configuration: DeepSeek API key (from env `DEEPSEEK_API_KEY`), base_url, model, languages, `concurrency`, `max_chunk_chars`, `max_retries` |
| **book::source** | Detect the file encoding (`chardetng`) and decode to UTF-8 (`encoding_rs`). Source `.txt` files are often GBK/GB18030 or Big5, not UTF-8 |
| **book::parser** | Split plain text into chapters by chapter-heading patterns (generic; Chinese `第N章` and Arabic today, more patterns + model-inferred delimiter planned). Normalize line endings. Parse the chapter number, produce a `ParseReport` (gaps, duplicates) |
| **book::fb2** | Parse FB2 (FictionBook XML) into sections → chapters (generic `<section>/<title>/<p>` walk). Language-agnostic heading-number extraction; merges split parts (1.1, 1.2 …) into whole chapters |
| **book::load** *(planned)* | Format-agnostic input: detect TXT vs FB2, decode, parse → chapters + meta |
| **book::chunker** | Fallback splitting of an over-long chapter into chunks on paragraph boundaries (never mid-sentence) |
| **reference** *(planned)* | Optional reference-translation subsystem: parse a professional translation (any format), align it to the source, and bootstrap a pinned glossary (names/lore) + a style exemplar via DeepSeek |
| **glossary** | Consistency subsystem: store terms, inject relevant ones into the prompt, auto-extract new ones, merge with conflict resolution |
| **retarget** | Propagate a glossary rename into already-translated text. Picks only the paragraphs that mention the old rendering (inflection-aware candidate match) and rewrites them via the model so declensions and gender/number agreement follow the new term |
| **translator::prompt** | Build system/user prompts: base instructions + mandatory term glossary + optional previous-chapter summary |
| **translator::deepseek** | DeepSeek HTTP client (`/chat/completions`), retry + backoff, handling 429/5xx |
| **state** | Persist progress and glossary in SQLite. Run resumption |
| **settings** | App-wide key-value settings (e.g. UI language) in a small SQLite DB in the app data dir, so preferences survive restarts |
| **export::txt** / **export::fb2** / **export::epub** / **export::pdf** | Assemble the result into the chosen output format: `.txt`, `.fb2` (per-chapter sections), `.epub` with a table of contents, or `.pdf` (cover page, contents with page numbers, clickable bookmarks) |
| **i18n** | Output-facing localization (the "Contents" heading, the "Chapter" label, fallback title/author). Strings live in `assets/locales.json` (`ru`/`en`/`zh`); a new output language is a JSON entry, not a code change |
| **commands** | Tauri commands — the Rust ↔ React bridge. Progress is pushed via events (`emit`) |

## Translation Pipeline

The main data flow — from file selection to a finished book:

```
book.{txt,fb2} (UTF-8 / GBK / Big5)
   │  book::load::load_book()                  detect encoding + format, decode
   │                                           parse → chapters, validate (gaps/dups)
   ▼
[ Chapter{index, number, title, body} × ~1350 ]
   │  state::Store::init_chapters()             idempotent insert, status=pending
   ▼
SQLite (progress.db)  ── running_summary, glossary, per-chapter status ──
   │  state::pending_chapters()                 resumable queue, in order
   ▼
SEQUENTIAL loop (chapter N, then N+1, …)         context carries forward:
   │
   │  1. glossary::relevant_terms(body)          terms occurring in this chapter
   │  2. running summary + prev chapter tail  ─┐ rolling context (from N-1)
   │  3. translator::prompt::user_prompt()      │ glossary + context + text
   │  4. deepseek.translate()  ── retry ──────> │ → DeepSeek API
   │  5. state::save_translation()              │ translation + status=done
   │  6. update running_summary (light call)  ──┘ fold chapter N into the synopsis
   │  7. glossary: extract new terms + merge     never overwrite the canon
   │  8. emit("progress", …)                     UI updates live
   ▼
export::txt / export::fb2 / export::epub         assemble the finished book
```

The loop is sequential so step 2 of chapter N+1 sees the summary produced by step
6 of chapter N. Both `running_summary` and the glossary live in SQLite, so an
interrupted run resumes with full context intact.

## Glossary Subsystem (translation consistency)

Without a shared dictionary an LLM translates each chapter independently and
mangles names (the same 王林 rendered three different ways), sect names,
locations, and cultivation terms. The glossary fixes this with a two-phase cycle
per chapter:

**Phase 1 — before translation.** Find the glossary terms that occur in the chapter
text and inject them into the user prompt as a mandatory dictionary: "translate
strictly like this."

**Phase 2 — after translation.** With a separate light request, extract new
names/terms (pairs of "source → chosen translation" + category) and merge them into
the glossary. On conflict, **the already-fixed canon wins**; the new term only bumps
its frequency counter. Terms with `pinned=true` (edited by hand from the UI) are
never overwritten by auto-extraction.

Categories (`TermKind`): `Person` · `Location` · `Organization` · `Term`.

The glossary keeps *terms* consistent; the **running summary** (see the pipeline
above and Core Principles) keeps the *narrative* consistent. Both are injected into
every chapter's prompt.

## Input & Output Formats

Input and output are pluggable; the core pipeline works on a `Vec<Chapter>`
regardless of format.

| Format | Input | Output | Notes |
|--------|:-----:|:------:|-------|
| **TXT** | ✅ | ✅ | Chapters by heading pattern (generic; model-inferred delimiter for unknown layouts) |
| **FB2** | ✅ | ⏳ | FictionBook XML; `<section>/<title>/<p>` structure, generic heading numbers |
| **EPUB** | — | ⏳ | Planned output with a table of contents |

Encoding on input is detected, not assumed (UTF-8 / GBK / GB18030 / Big5, …).

## Reference Translation (optional)

If the user supplies a professional translation of the same book (any supported
format, e.g. an FB2 from a translation site), it is used as a source of truth for
**names, lore, and style** — but never hardcoded to a specific title:

1. **Parse** the reference into chapters (via the same format parsers).
2. **Align** it to the source by reading order over the overlapping opening
   chapters; the model can confirm a match when numbering diverges. (Reference
   editions often split a chapter into parts — these are merged back.)
3. **Bootstrap the glossary** — on a sample of aligned chapter pairs (source +
   professional translation, ~30 by default), run term extraction to get
   `source → professional rendering` pairs and add them as **pinned** canon
   (human-quality, never overwritten by auto-extraction).
4. **Style exemplar** — a short professional excerpt is injected into the
   translation prompt as a few-shot style reference.

Where no reference is supplied, the pipeline runs on the auto-grown glossary alone.

## State Storage (SQLite)

A single `progress.db` next to the book. Schema sketch:

| Table | Columns | Purpose |
|-------|---------|---------|
| **chapters** | `index PK, title, source, status, translated, updated_at` | Chapters and their translations. `status`: `pending` / `in_progress` / `done` / `failed` |
| **glossary** | `source PK, target, kind, frequency, pinned` | Canonical term translations |
| **meta** | `key PK, value` | Book path, run parameters, schema version, and the **running summary** (story-so-far, updated after each chapter) |

Resumption: on start, take `chapters WHERE status IN ('pending','failed')`. Any
`in_progress` left over from a crash is reset to `pending` on start.

## Error Handling and Limits

- **Retry:** network errors, `429`, `5xx` → exponential backoff, up to `max_retries`.
- **Order:** chapters run **sequentially** by default so the running summary from
  chapter N feeds chapter N+1 (`concurrency = 1`). An optional parallel mode raises
  `concurrency` for speed but drops cross-chapter context.
- **Failure isolation:** a chapter that fails after retries is marked `failed` and
  the run continues. `failed` chapters can be re-run separately.
- **Cost:** estimated before a run from character count × DeepSeek pricing
  (see `DECISIONS.md`).

## External Interfaces

- **DeepSeek API** — OpenAI-compatible `POST /chat/completions`, models
  `deepseek-chat` (translation) and optionally `deepseek-reasoner`. Auth:
  `Authorization: Bearer $DEEPSEEK_API_KEY`.
- **Filesystem** — input: `.txt` / `.fb2`; output: `.txt` / `.fb2` (`.epub` planned);
  state: `.db`.

## Frontend Commands (Tauri IPC)

| Command | Purpose |
|---------|---------|
| `parse_book(path)` | Parse the book, init `progress.db`, return `BookSummary` |
| `start_translation()` | Start/resume translation (only `pending`/`failed`) |
| `pause_translation()` | Pause after current chapters finish |
| `get_progress()` | Current progress (also pushed via the `progress` event) |
| `get_glossary()` | The whole glossary for the UI table |
| `update_term(term)` | Add or edit/pin a term (`pinned=true`) |
| `delete_term(source)` | Remove a term from the glossary |
| `retarget_terms(changes)` | Propagate one or more renames into the existing translation (background job; `retarget_progress`/`retarget_done` events) |
| `export_book(out_dir, formats)` | Export to TXT/EPUB, return file paths |
| `get_setting(key)` / `set_setting(key, value)` | Read/write a durable app-wide setting (e.g. UI language) |
| `delete_project(project_id)` | Delete a project and all its data |
| `export_project(project_id, out_path)` / `import_project(project_id, archive_path)` | Save/open a project as a portable `.bcproj` archive |

Projects are isolated: `AppState` holds one `Session` per `project_id`, each with its
own data directory (`projects/<id>/`), progress DB, cancel/running flag and console
log, so several books can translate in parallel. Every project-scoped command takes a
`project_id`, and progress/job events carry it. See `docs/PROJECT_ISOLATION.md`.

## Deliberately Out of Scope for v1

- EPUB/PDF/HTML **input** — TXT and FB2 first; EPUB output planned.
- Cloud sync, multi-user mode — not planned (this is a local tool).

The language pair and output format are configurable by design; the primary test
run is Chinese → Russian.
