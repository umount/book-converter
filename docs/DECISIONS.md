# book-converter — Design Decisions

> Last updated 2026-07-17

Record of the key choices and their rationale. Newest first.

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

## Output formats: TXT + EPUB

**Decision:** produce both a plain `.txt` and an `.epub` with a per-chapter table of
contents.

**Why:** `.txt` is the simplest artifact and a safe fallback; `.epub` is the
comfortable reading format with navigation. Both are cheap to generate from the same
translated chapters.
