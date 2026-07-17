# book-converter — Architecture

> Last updated 2026-07-17 — initial version, in sync with the code scaffold.

## Overview

book-converter is a desktop app that translates **large** books from Chinese to
Russian via the **DeepSeek API**. The target case is the web novel `光阴之外`
(author 耳根): ~11 MB of text, **990 chapters**, ~4300 Chinese characters per chapter.

The hard part is not the UI — it is the **translation pipeline**: a book cannot be
sent to the model in one shot, translation runs as hundreds of requests, the job
takes a long time, and it must survive restarts. A separate challenge is
**consistency**: the protagonist's name, locations, and setting terminology must be
translated identically across all 990 chapters.

## Core Principles

- **Chunk by chapter, not by bytes.** The natural boundary is the `第N章` marker.
  A whole chapter fits in one request (`deepseek-chat` context is 64K tokens).
  Character-based splitting is only a fallback for abnormally long chapters, and it
  splits on paragraph boundaries.
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
| **book::parser** | Split `.txt` into chapters by `第[一二…]章` markers. Normalize line endings (CRLF/CR → LF) |
| **book::chunker** | Fallback splitting of an over-long chapter into chunks on paragraph boundaries (never mid-sentence) |
| **glossary** | Consistency subsystem: store terms, inject relevant ones into the prompt, auto-extract new ones, merge with conflict resolution |
| **translator::prompt** | Build system/user prompts: base instructions + mandatory term glossary + optional previous-chapter summary |
| **translator::deepseek** | DeepSeek HTTP client (`/chat/completions`), retry + backoff, handling 429/5xx |
| **state** | Persist progress and glossary in SQLite. Run resumption |
| **export::txt** / **export::epub** | Assemble the result into a single `.txt` and into an `.epub` with a per-chapter table of contents |
| **commands** | Tauri commands — the Rust ↔ React bridge. Progress is pushed via events (`emit`) |

## Translation Pipeline

The main data flow — from file selection to a finished book:

```
book.txt
   │  book::parser::parse_chapters()           normalize CRLF/CR, split on 第N章
   ▼
[ Chapter{index, title, body} × ~990 ]
   │  state::Store::init_chapters()             idempotent insert, status=pending
   ▼
SQLite (progress.db)
   │  state::pending_chapters()                 resumable queue
   ▼
worker pool (concurrency=N)  ───────────────┐   per chapter:
   │                                         │
   │  1. glossary::relevant_terms(body)      │   which terms occur in this chapter
   │  2. book::chunker::split_chapter()      │   usually 1 chunk; fallback if long
   │  3. translator::prompt::user_prompt()   │   term glossary + text
   │  4. deepseek.translate()  ── retry ───> │   → DeepSeek API
   │  5. state::save_translation()           │   translation + status=done
   │  6. glossary::extract_terms()           │   new names/terms (light request)
   │  7. glossary::merge() + save_glossary() │   never overwrite the canon
   │                                         │
   └── emit("progress", …) ──────────────────┘   UI updates without blocking
   ▼
export::txt / export::epub                       assemble the finished book
```

## Glossary Subsystem (translation consistency)

Without a shared dictionary an LLM translates each chapter independently and
mangles names ("Ван Линь" / "Ванлинь" / "Wang Lin"), sect names, locations, and
cultivation terms. The glossary fixes this with a two-phase cycle per chapter:

**Phase 1 — before translation.** Find the glossary terms that occur in the chapter
text and inject them into the user prompt as a mandatory dictionary: "translate
strictly like this."

**Phase 2 — after translation.** With a separate light request, extract new
names/terms (pairs of "source → chosen translation" + category) and merge them into
the glossary. On conflict, **the already-fixed canon wins**; the new term only bumps
its frequency counter. Terms with `pinned=true` (edited by hand from the UI) are
never overwritten by auto-extraction.

Categories (`TermKind`): `Person` · `Location` · `Organization` · `Term`.

Optional: a short "previous-chapter summary" can be added to the prompt for
narrative continuity (see `ROADMAP.md`, extensions stage).

## State Storage (SQLite)

A single `progress.db` next to the book. Schema sketch:

| Table | Columns | Purpose |
|-------|---------|---------|
| **chapters** | `index PK, title, source, status, translated, updated_at` | Chapters and their translations. `status`: `pending` / `in_progress` / `done` / `failed` |
| **glossary** | `source PK, target, kind, frequency, pinned` | Canonical term translations |
| **meta** | `key PK, value` | Book path, run parameters, schema version |

Resumption: on start, take `chapters WHERE status IN ('pending','failed')`. Any
`in_progress` left over from a crash is reset to `pending` on start.

## Error Handling and Limits

- **Retry:** network errors, `429`, `5xx` → exponential backoff, up to `max_retries`.
- **Concurrency:** `concurrency` simultaneous requests (default 4). Bounds API load
  and helps stay within rate limits.
- **Failure isolation:** a chapter that fails after retries is marked `failed` and
  the run continues. `failed` chapters can be re-run separately.
- **Cost:** estimated before a run from character count × DeepSeek pricing
  (see `DECISIONS.md`).

## External Interfaces

- **DeepSeek API** — OpenAI-compatible `POST /chat/completions`, models
  `deepseek-chat` (translation) and optionally `deepseek-reasoner`. Auth:
  `Authorization: Bearer $DEEPSEEK_API_KEY`.
- **Filesystem** — input: `.txt`; output: `.txt` + `.epub`; state: `.db`.

## Frontend Commands (Tauri IPC)

| Command | Purpose |
|---------|---------|
| `parse_book(path)` | Parse the book, init `progress.db`, return `BookSummary` |
| `start_translation()` | Start/resume translation (only `pending`/`failed`) |
| `pause_translation()` | Pause after current chapters finish |
| `get_progress()` | Current progress (also pushed via the `progress` event) |
| `get_glossary()` | The whole glossary for the UI table |
| `update_term(term)` | Manually edit/pin a term (`pinned=true`) |
| `export_book(out_dir, formats)` | Export to TXT/EPUB, return file paths |

## Deliberately Out of Scope for v1

- Other input formats (EPUB/PDF/HTML) — `.txt` only for now.
- Other language pairs — the architecture allows them, but the v1 goal is
  Chinese → Russian.
- Cloud sync, multi-user mode — not planned (this is a local tool).
