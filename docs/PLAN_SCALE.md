# Plan: scale, correctness, and cost of the translation loop

> Written 2026-08-21. Tracks the work started after the architecture review of
> commit `599ca6b`. Each phase below lands as one commit.

## Why

Three problems reported from real use on a 1354-chapter book, plus the findings
of an architecture review of the same codebase:

1. **A 10K-term glossary freezes the app.** The whole glossary is loaded on every
   refresh, rendered as 10K live DOM rows with inputs, scanned twice per chapter
   render for highlighting, and rewritten in full on every single-term edit.
2. **Language repair re-sends the whole chapter.** When a translation keeps words
   in the wrong language, `enforce_target_language` ships the entire chapter back
   to the model, up to twice. It is expensive, slow, and risky enough that the
   code already carries a guard against the model mangling the whole text.
3. **The glossary is persisted after every chapter.** `enrich_glossary` writes the
   full term list once per chapter. It should be merged and written once per run.

Plus, from the review: `Store::open` mutates data, SQLite runs without WAL or a
busy timeout, the project registry lives only in `localStorage`, the orchestrator
has no test seam, and `state/mod.rs` is a 1092-line god-object.

## Principles kept

- **Send the model the smallest correct unit.** Chapter for a first translation,
  the affected lines for a repair. Never a whole chapter to fix three words.
- **Persist per run, not per item, where the item is cheap to redo.** The glossary
  is derived data: losing the last chapter's terms costs one extraction, not work.
  Chapter translations stay per-chapter, they are expensive and irreplaceable.
- **The backend owns the data, the frontend owns the view.** Paging, filtering and
  relevance live in SQL, not in a React `useMemo` over 10K rows.
- **Reading the database must never write to it.**

## Phases

### 0. Secret hygiene

`.claude/settings.local.json` holds a live DeepSeek key in an allow rule and is
not ignored by git. Add it to `.gitignore`. The key itself must be rotated by
hand, it is outside what this repo can do.

### 1. SQLite foundation

- `Store::open` becomes pure: schema, migrations, pragmas. Nothing else.
- New `Store::recover()` performs the `in_progress -> pending` crash reset, called
  once from `open_project` and once at job start.
- Pragmas on every connection: `journal_mode=WAL`, `busy_timeout=5000`,
  `synchronous=NORMAL`.

Fixes, as a side effect, the `chapter_busy` guard in `update_chapter_translation`,
which today can never fire because the read that checks it has already cleared the
status it looks for. A manual edit made during a run is currently overwritten in
silence.

### 2. Structured model replies

Today the chapter title is recovered by guessing: `split_title_body` takes the
first line of the reply, strips `#` and `*`, and falls back to the source title
when the reply happens to be a single line. Every prompt says "output only the
translation" and the parser hopes the model obeyed. The same reply also carries
no way to say "this chunk had no title".

The fix is to make the reply's shape explicit, but **not** by wrapping chapter
bodies in JSON.

**Why not JSON for chapter bodies.** `DeepSeekClient::translate` recovers from
the output token limit by detecting `finish_reason = length` and asking the model
to continue (up to `MAX_CONTINUATIONS`). A truncated JSON document cannot be
parsed, and a continued one cannot be reliably re-joined, so JSON mode would
trade a rare title-parsing mistake for a hard failure on exactly the long
chapters the continuation logic exists to save. JSON also forces the whole
chapter through string escaping, which costs tokens and invites escaping bugs on
text full of quotes and newlines.

**Chapter translation uses a delimited envelope** instead, which survives
truncation because a continuation simply appends to the body:

```
<<<TITLE>>>
Глава 523. За гранью меча
<<<BODY>>>
Линь Хань шагнул вперёд.
…
```

- `translator::reply` owns the envelope: the instruction text, a strict parser,
  and a lenient fallback to today's heuristic when a model ignores the format, so
  a non-conforming reply degrades instead of failing.
- Chunks after the first ask for `<<<BODY>>>` only, which removes the current
  "chunk.part == 0" special case in the orchestrator.
- `split_title_body` stays as that fallback and keeps its tests.

**The small structured calls do use real JSON**, with DeepSeek's
`response_format: {"type": "json_object"}`, because they are short, bounded, and
have nothing to truncate:

- term extraction, which today asks for a bare array and slices `[ … ]` out of
  possibly-decorated prose (`slice_json_array`). It becomes `{"terms": [...]}`,
  which is what `json_object` mode requires at the top level.
- the line repair introduced in the next phase, which returns
  `{"lines": [{"n": 12, "text": "…"}]}` so lines are spliced back by number
  rather than by counting lines in a free-text reply.

This needs a `response_format` field on `ChatRequest` and a `translate_json`
entry point on the client, alongside the existing plain `translate`.

**Also fixed here:** `glossary::build_extraction_prompt` hardcodes
"Chinese→Russian" and asks for Chinese source forms and Russian renderings, in a
tool whose stated design is language-pair agnostic. It takes the pair from
`Config` like every other prompt.

### 3. Line-scoped language repair

`enforce_target_language` keeps its detection (`textutil::foreign_fragments`) and
its repair budget, but changes what it sends:

- map each leftover fragment to the lines that contain it;
- send only those lines, numbered, in one request;
- splice the returned lines back into the chapter by the numbers they carry,
  using the JSON line reply defined in the previous phase;
- validate per line (a line that comes back empty or wildly shorter is dropped,
  keeping the original), instead of validating the whole chapter at once.

A chapter with three bad words in two paragraphs then costs two paragraphs of
tokens, not a full chapter, and an unrelated paragraph can no longer be damaged
by a repair pass.

### 4. Glossary write path

- `update_term` / `delete_term` stop doing read-all + write-all. Single-row upsert
  and delete.
- `Store::save_glossary` keeps its bulk form for the batch path, but the
  orchestrator no longer calls it per chapter.
- The orchestrator accumulates extracted terms in memory across the run and merges
  and persists once, at the end of the run, on pause, and on cancel, so an
  interrupted run does not lose the terms it already learned.
- Single-chapter translation (`run_one`) persists at the end of its own run, which
  is the same rule applied to a run of one.

### 5. Glossary read path

- New command `get_glossary_page(project_id, query, kind, offset, limit)` returning
  `{ total, terms }`, with filtering and ordering done in SQL and an index on
  `frequency`.
- The glossary table becomes virtualized and paged: it renders a window, not 10K
  rows, and the filter box queries the backend.
- Reader highlighting stops receiving the whole glossary. New command
  `chapter_terms(project_id, index)` returns only the terms whose source appears
  in that chapter, reusing `glossary::relevant_terms`, which the translation
  prompt already uses for exactly this reason.

### 6. A test seam for the orchestrator

`Orchestrator` takes a `Translate` trait instead of a concrete `DeepSeekClient`.
`DeepSeekClient` implements it. Tests then drive the whole loop against a scripted
fake: sequential order, resume, the repair path from phase 2, and the batched
glossary persistence from phase 3.

### 7. Projects recoverable from disk

`list_projects` scans `projects/*/project.json` and reports id, name, source path
and chapter counts. The frontend reconciles its `localStorage` registry with that
list on startup, so clearing browser storage no longer strands project data with
no way back except a `.bcproj` import.

### 8. Store split and blocking work off the executor

- `state/mod.rs` splits into `state/{mod,schema,chapters,glossary,meta,search}.rs`,
  same public API.
- `search_book`, `replace_in_book` and `export_book` move their database and text
  work onto `spawn_blocking`, matching what `commands/mod.rs` already claims the
  design does.
- Book-wide search streams chapter rows instead of collecting the whole book into
  a `Vec<String>` first.

### 9. Frontend workspace context

`App.tsx` currently builds a mutable ref of 25 callbacks and hands it to
`useProjects`, and `useTranslationJob` mirrors 10 more props into refs. Replace
both with a workspace context that hooks read from, which is what keeps the
project activation order implicit today.

### 10. Documentation

Sync `ARCHITECTURE.md` with the commands added since 2026-07-22
(`translate_chapter`, `update_chapter_translation`, `set_chapter_prompt`,
`set_chapter_context`, plus everything added here), record the new decisions in
`DECISIONS.md`, and drop the "Chinese to Russian" wording from `lib.rs` and
`Cargo.toml`, which contradicts the language-pair-agnostic design.
