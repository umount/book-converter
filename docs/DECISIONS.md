# book-converter — Design Decisions

> Last updated 2026-07-17

Record of the key choices and their rationale. Newest first.

---

## Sequential translation with a rolling context (over parallel)

**Decision:** translate chapters in order, carrying a compact **running summary**
of the story so far (plus the previous chapter's closing lines) into each chapter's
prompt. Default `concurrency = 1`. A parallel mode is available but drops
cross-chapter context.

**Why:** translating each chapter in isolation loses narrative continuity — ongoing
scenes, who characters are, unresolved threads, tone shifts. Feeding a running
summary keeps meaning consistent across a 1350-chapter arc. The user asked for this
explicitly. The summary is updated after each chapter with a light call and stored
in `meta.running_summary`, so an interrupted run resumes with context intact.

**Trade-off:** slower than a parallel pool. Accepted — coherence matters more than
raw speed for a long novel; parallel mode remains for users who want throughput.

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

## Consistency via an auto-growing glossary

**Decision:** maintain a glossary of canonical translations (names, locations,
organizations, terms). Inject relevant terms into each chapter's prompt; extract and
merge new terms after each chapter; canon/pinned entries win on conflict.

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

**Config:** API key from env `DEEPSEEK_API_KEY` (never committed). `deepseek-reasoner`
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
