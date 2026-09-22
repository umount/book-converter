# book-converter — Design Decisions

> Last updated 2026-09-21

Record of the key choices and their rationale. Newest first.

---

## One job lease per project (`jobs::lease`)

**Decision:** `AppState::begin_job` is called only from `jobs::lease`. The
returned `JobSlot` releases the project on `Drop`. Translation, retarget,
harvest, bootstrap, reset, book-wide replace, and reference re-seed all take
this lease. Hand-editing a chapter does not.

**Why:** harvest and bootstrap used to check `running` (or not check at all) and
then rewrite the glossary while the orchestrator could also merge terms. The
lease makes the busy flag atomic and lasts for the whole operation. The error
code is always `job_running`.

---

## Language pair and model stay app-wide

**Decision:** source/target language and the DeepSeek model remain Settings, not
per-project fields. The assistant snapshot labels them `app_settings.*` so it
does not pretend they belong to the open book.

**Why:** `Config::load` and the translation job already read them from the
settings DB. Per-project language would be the right long-term shape (two open
books with different pairs) but it is a separate migration of config, prompts,
and the orchestrator, not an assistant-only change.

---

## Framed chapter replies, JSON only where a reply cannot truncate

**Decision:** a chapter translation comes back inside `<<<TITLE>>>` /
`<<<BODY>>>` markers, parsed by `translator::reply`. The short, bounded calls
(term extraction, language repair) use real JSON via DeepSeek's
`response_format: json_object`.

**Why:** the title used to be recovered by guessing, taking the reply's first
line and stripping markdown. That eats a short opening paragraph when the model
skips the title, and promotes prose when it writes one differently than expected.

**Why not JSON for the chapter body:** the client survives the output token limit
by continuing a reply whose `finish_reason` is `length`. Truncated JSON cannot be
parsed and continued JSON cannot be rejoined, so JSON would convert a rare title
slip into a hard failure on exactly the long chapters that continuation exists to
rescue. It also pushes the whole chapter through string escaping. Markers cost
nothing to escape, and a continuation simply appends to the body. The old
heuristic is kept as a fallback, so a reply that ignores the format degrades
rather than fails.

---

## Language repair is line-scoped, not chapter-scoped

**Decision:** when a translation keeps words in the wrong language, only the
lines containing them are sent back to the model, batched under a character cap,
and spliced into place by line number from a JSON reply. The chapter title is
line 0 of that list.

**Why:** the repair pass re-sent the whole chapter, up to twice, to fix a handful
of words. On a book where the pass fires often that is a second and third full
translation cost per chapter. It was also unsafe: a model asked to rewrite four
thousand characters "changing nothing else" sometimes summarises instead, which
is why the old code carried a guard that discarded a repair that came back much
shorter. That guard is now per line, so a derailed reply can only lose its own
line, and correct paragraphs are never at risk because they are never sent.

---

## The glossary is written per chapter, but only what changed

**Decision:** terms extracted from a chapter are merged and persisted before the
next chapter starts. What changed is that only the rows this chapter touched are
written, instead of the whole term list, and that a term edited in the UI during
a run keeps the rendering the user chose (the flush re-reads the row and writes
back only the frequency it counted).

**Why:** a name first seen in chapter 40 has to be in the dictionary by the time
chapter 41 is translated. That is the entire purpose of growing a glossary
mid-run, so the per-chapter write stays. The cost problem was never the timing,
it was rewriting tens of thousands of rows to record one new term. The UI had a
matching bug: it only refetched the glossary when the run finished, so through a
long run the table looked frozen.

---

## The glossary is paged by the database, not filtered in the UI

**Decision:** the glossary view asks for one filtered, ordered window at a time
(`get_glossary_page`) and renders only the rows in view. The reader asks for the
terms occurring in the open chapter (`chapter_terms`) rather than receiving all
of them.

**Why:** a long book's glossary reaches tens of thousands of terms. Shipping all
of them to the frontend and rendering every match as a live row, each with two
inputs and a select, froze the app; so did rescanning the whole list against the
chapter text on every reader render. Matching uses a Unicode-aware lowercase
registered on the connection, because SQLite's own `lower()` and `LIKE` fold case
for ASCII only, which would leave the filter useless in the scripts this tool
actually works in.

---

## Reading the database never writes to it

**Decision:** `Store::open` applies schema, migrations and pragmas, and nothing
else. Crash recovery is an explicit `Store::recover()`, called on project
activation and at job start.

**Why:** `open` used to reset every `in_progress` chapter to `pending`, and it is
called from more than thirty places, most of them reads the UI polls. The status
of the chapter being translated was therefore erased almost as soon as it was
set, which broke the guard that refuses a hand edit to a chapter mid-translation:
the read that checks it had already cleared the value it looks for, so the edit
was silently overwritten when the translator saved.

---

## Sequential translation with a rolling context (over parallel)

**Decision:** translate chapters in order, carrying a compact **running summary**
of the story so far (plus the previous chapter's closing lines) into each chapter's
prompt. Translation is always sequential. Parallel chapter translation is out of
scope (it would drop cross-chapter context).

**Why:** translating each chapter in isolation loses narrative continuity — ongoing
scenes, who characters are, unresolved threads, tone shifts. Feeding a running
summary keeps meaning consistent across a 1350-chapter arc. The user asked for this
explicitly. The summary is updated after each chapter with a light call and stored
in `meta.running_summary`, so an interrupted run resumes with context intact.

**Trade-off:** slower than a parallel pool. Accepted — coherence matters more than
raw speed for a long novel. A future parallel mode would be an explicit opt-in that
disables the rolling summary; it is not implemented.

---

## Universal converter, not book-specific

**Decision:** the tool is a general book translator. Language pair is config;
formats are pluggable; chapter detection uses generic patterns and, when they
fail, asks the model to infer the delimiter. Nothing is hardcoded to one title.

**Why:** the user explicitly wants a reusable converter, not a one-off script for
`光阴之外`. Deterministic code handles reliable structure (XML, encoding, obvious
markers); the model handles the fuzzy/semantic parts (unknown delimiters,
reference alignment, names/lore/style).

---

## FB2 as an input and output format

**Decision:** support FB2 (FictionBook XML) on input and output, alongside TXT;
EPUB output later. Parsing is a generic `<section>/<title>/<p>` walk with a
language-agnostic heading-number extractor; split parts (1.1, 1.2 …) merge into
whole chapters.

**Why:** professional translations and many e-book sources ship as FB2. Requested
by the user. `quick-xml` is a pure-Rust, dependency-light parser.

---

## Reference translation → pinned glossary + style (canon+style mode)

**Decision:** when a professional translation is supplied, extract names/lore from
aligned source↔reference chapter pairs (a ~30-chapter sample by default) into a
**pinned** glossary, and inject a professional excerpt as a style exemplar. All
chapters are still machine-translated for a uniform style; the reference is a
knowledge source, not the output.

**Why (chosen over reusing the professional chapters as output):** the user wants
consistent style across the whole book. Reusing the pro text verbatim for chapters
1–514 and machine-translating the rest would create a visible style seam. Mining
the reference for canon + style gives the machine translation the same names and
tone without the discontinuity. The ~30-chapter sample keeps the one-time
bootstrap cheap; it can be extended later.

**Alignment** is by reading order over the overlapping opening chapters (general,
no per-book numbering assumptions); the model can confirm matches if numbering
diverges.

---

## Chunk by chapter (not by fixed token/byte size)

**Decision:** the unit of translation is a whole chapter, delimited by the `第N章`
marker. Character-based splitting exists only as a fallback for abnormally long
chapters, splitting on paragraph boundaries.

**Why:** a chapter (~4300 Chinese chars) fits comfortably in one request
(`deepseek-chat` context is 64K tokens). Chapters are natural narrative units, so the
model gets coherent context. Splitting mid-chapter (worse, mid-sentence) degrades
quality and complicates reassembly.

---

## A chapter's pictures live in its text, as `[[img:<id>]]` lines

**Decision:** an EPUB page is parsed into typed blocks (text / image / caption),
but `chapters.source` remains the single source string the pipeline works on,
with each picture standing in it as a line `[[img:<content hash>]]`. The image
files are copied into the project (`assets/`) and served to the webview over a
`bookasset://` scheme. A page with no words gets its own status, `skipped`.

**Why:** every stage downstream of parsing — chunker, prompts, glossary, search,
find/replace, retarget, the reader's autosave, export — is built on a chapter
being a string. A picture is positional information, and a marker line is the
cheapest way to carry a position through all of them: it survives translation
(the model is told to copy it, and `restore_markers` repairs what it drops),
book-wide replace, and a hand edit in the reader, with no stage needing to learn
about blocks. Pages that are only pictures would otherwise be imported as empty
chapters and sit in the queue forever, costing API calls on nothing and holding
progress below 100%.

**Alternatives considered:** (a) blocks as the only truth, every stage rewritten
to walk them — a large change to earn nothing for the text-only books that are
the common case; (b) HTML kept verbatim as the chapter body — the model would
translate the markup and the reader would have to sanitize it; (c) images as
data URLs in the IPC payload — a manga page is megabytes, and the reader would
re-encode every page it scrolls past.

---

## Consistency via an auto-growing glossary

**Decision:** maintain a glossary of canonical translations (names, locations,
organizations, terms). Before each chapter, inject only terms that occur in that
chapter’s **original** text; after translation, extract and merge new terms;
canon/pinned entries win on conflict. The stored glossary may grow large; the
per-chapter prompt does not receive the full list.

**Why:** the core quality problem in long-form LLM translation is drift — the
protagonist's name comes out three different ways across chapters. A shared,
enforced dictionary is the standard fix. Raised explicitly by the author as a
must-have.

**Alternatives considered:** (a) one giant request — impossible at 11 MB; (b) rolling
full-text context — too expensive/token-heavy; (c) manual glossary only — doesn't
scale to 990 chapters. Chosen: auto-extract + manual pinning for overrides.

---

## Persistent progress in SQLite

**Decision:** store every chapter's status and translation, plus the glossary, in a
local SQLite `progress.db`. Runs are resumable; only `pending`/`failed` chapters are
processed.

**Why:** 990 chapters × network requests is a long, interruptible job. Losing
progress on a crash or restart is unacceptable. SQLite is embedded, transactional,
and zero-ops.

**Alternatives:** flat JSON/files — weaker on concurrent updates and querying by
status. SQLite via `rusqlite` (bundled) wins.

---

## UI stack: Tauri + React (not iced)

**Decision:** Tauri v2 desktop shell with a React 18 + TypeScript frontend; all logic
in a Rust core library called through Tauri commands.

**Why:** the UI is data-heavy (editable glossary table, 990-row chapter list,
original↔translation preview). Web tech handles tables/lists/editing far more easily
and looks better than hand-built iced widgets. The heavy lifting stays in Rust
regardless, so Tauri keeps the core language while giving a comfortable frontend.
The author has TS experience; no prior Rust — a familiar frontend lowers risk.

**Trade-off:** adds a JS/TS toolchain and an IPC boundary. Accepted. Original plan
was iced (pure Rust); switched after weighing the UI shape.

**Consequence:** on Linux, Tauri needs `webkit2gtk-4.1` and related system libs
(see `README.md` § Prerequisites).

---

## Provider: DeepSeek API

**Decision:** DeepSeek (`deepseek-chat`), OpenAI-compatible `/chat/completions`.

**Why:** requested by the author; strong Chinese→Russian quality; large context;
low cost per token for a 990-chapter job. The author already uses DeepSeek in another
project (napartner), so it's a known quantity.

**Config:** API key entered in Settings and stored in the settings DB, falling
back to env `DEEPSEEK_API_KEY` for a headless run (never committed either way).
It is the one setting where the UI wins over the environment, because it is the
one an ordinary user has to provide; everywhere else the environment wins so an
operator can pin a value. The key is never returned to the frontend once saved,
only a masked hint, and `Config`'s `Debug` redacts it. `deepseek-reasoner`
left as an option for hard passages.

---

## Target: Chinese → Russian, translation-only output

**Decision:** translate to Russian; output contains the translation only (no
bilingual/interleaved original).

**Why:** author's stated preference. Bilingual output would roughly double file size
with no benefit for a reading use case.

---

## Output formats: TXT, FB2, EPUB, PDF

**Decision:** export to plain `.txt`, `.fb2`, `.epub` (per-chapter table of contents),
and `.pdf` (cover page, contents with page numbers, clickable bookmarks). PDF is
rendered with `genpdf` for layout/pagination and patched with `lopdf` for the outline
and a Unicode document title.

**Why:** `.txt` is the simplest artifact and a safe fallback; `.fb2`/`.epub` are the
comfortable reading formats with navigation; `.pdf` is the print/share format. All are
cheap to generate from the same translated chapters.

---

## Output localization lives in data, not code

**Decision:** output-facing strings (the "Contents" heading, the "Chapter" label,
fallback title/author) are looked up through the `i18n` module from
`assets/locales.json` (`ru`/`en`/`zh`), keyed by a normalized language code. Code calls
`i18n::label(lang, key)`; it never embeds a translated string or a per-language branch.

**Why:** the converter is language-pair agnostic and will grow more output languages
(English, Chinese). Adding one should be a JSON entry with an English fallback, not a
hunt for hardcoded literals across `pdf.rs`/`commands.rs`.

---

## PDF text via bundled pdfium (not pure-Rust only)

**Decision:** extract PDF text with Google's **pdfium** via `pdfium-render`, shipping
a prebuilt pdfium dynamic library per platform (fetched by `make fetch-pdfium` from
bblanchon/pdfium-binaries, bundled as a Tauri resource). Poppler `pdftotext` and the
pure-Rust `pdf-extract`/`lopdf` remain fallbacks.

**Why:** many real PDFs subset fonts with custom encodings and no ToUnicode map. The
pure-Rust extractors then return empty or shifted, space-less garbage (the sample
"The Design of Web APIs" is one such file); pdfium and Poppler decode them correctly.
Poppler alone is not portable (absent on stock macOS/Windows), so a bundled pdfium
gives correct extraction everywhere while the fallbacks cover the case where the
library is missing.
