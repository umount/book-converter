# book-converter — Roadmap / Work Plan

> Last updated 2026-07-17

Staged plan from scaffold to a finished translated book. Each stage is independently
verifiable. Since the author is new to Rust, early stages double as a way to get
comfortable with the language on small, testable pieces before the UI.

## Legend

- `[ ]` todo · `[~]` in progress · `[x]` done
- **Verify** — how to confirm the stage works.

---

## Stage 0 — Project scaffold `[x]`

- [x] `git init`, Tauri layout (`src-tauri/` core + React frontend)
- [x] Rust module skeleton (`config`, `book`, `glossary`, `translator`, `state`, `export`, `commands`)
- [x] `Cargo.toml`, `tauri.conf.json`, capabilities, frontend scaffold (Vite + React + TS)
- [x] Docs: `ARCHITECTURE.md`, `ROADMAP.md`, `DECISIONS.md`, `README.md`

**Verify:** structure reviewed; `docs/` describes the design.

---

## Stage 1 — Environment & build `[ ]`

- [ ] Install system deps for Tauri on Linux: `webkit2gtk-4.1`, `librsvg`, `build-essential`, etc. (currently **missing** — see `README.md` § Prerequisites)
- [ ] `npm install`; install `tauri-cli` (`cargo install tauri-cli` or npm devDep)
- [ ] `cargo build` on `src-tauri` (downloads crates, compiles core)
- [ ] `npm run tauri dev` opens an empty window

**Verify:** the app window launches; no build errors.

---

## Stage 2 — Book parsing (`book::source` + `book::parser`) `[x]`

- [x] Encoding layer (`book::source`): detect (`chardetng`) + decode (`encoding_rs`) UTF-8 / GBK / GB18030 / Big5, report the detected encoding
- [x] Normalize line endings (CRLF/CR → LF)
- [x] Regex for chapter markers — both Arabic (`第1章`) and Chinese (`第一章`) numerals
- [x] Build `Chapter { index, number, title, body }` between markers
- [x] Chinese-numeral → int parser (note: Chinese digits are not contiguous in Unicode)
- [x] Parse header metadata (title `《...》`, author `作者：...`, declared count)
- [x] `validate()` → `ParseReport`: gaps, duplicates, declared vs actual
- [x] Unit tests (11) + verified against 3 real books

**Verified** against three real books:
- `光阴之外.txt` — UTF-8, Chinese numerals: 976 chapters, truncated at #984, 6 duplicates, 14 gaps.
- `光阴之外⊙完本.txt` — UTF-8, Arabic numerals: **1350 chapters (clean), #1354 end** — the edition to translate.
- `末世之黑暗召唤师….txt` — **GBK**, Chinese numerals up to #1619: 1662 entries, first 43 chapters duplicated.

**Note (feeds later stages):** real scraped books have gaps/duplicates. A dedup/gap
policy belongs in the state/orchestration stage; the parser stays faithful and
surfaces a `ParseReport` for the UI to warn on.

---

## Stage 3 — State store (`state`) `[x]`

- [x] Open/create SQLite, apply schema (`chapters`, `glossary`, `meta`)
- [x] `init_chapters` — idempotent insert (never clobbers existing translations)
- [x] `pending_chapters`, `save_translation`, `set_status`, `stats`, `translated_chapters`
- [x] Reset stray `in_progress` → `pending` on startup (crash recovery)
- [x] Glossary load/save (upsert) + `meta` get/set
- [x] 7 unit tests incl. a real reopen-resumption test

**Verified:** end-to-end on `光阴之外⊙完本.txt` — parse → 1350 chapters into the
store; after translating 5 and failing 1, `stats` reports `done=5 failed=1
pending=1345` and a reopened DB requeues the `in_progress` chapter.

---

## Stage 4 — DeepSeek client + prompts (`translator`) `[x]`

- [x] `config::load` — key from env `DEEPSEEK_API_KEY` (+ `DEEPSEEK_MODEL`/`DEEPSEEK_BASE_URL`)
- [x] `deepseek::translate` — `POST /chat/completions`, parse response
- [x] Retry with exponential backoff on 429/5xx/timeouts (retryable vs fatal classification)
- [x] `prompt::system_prompt` / `user_prompt` (mandatory term dictionary + optional summary + text)
- [ ] Cost/size estimation helper (char count × pricing) — deferred to the UI stage

**Verified:** a real DeepSeek call translated a Chinese snippet to fluent Russian
and honored the injected glossary term (`王林` → "Ван Линь"). The API key is read
from env only and never touches the repo (`.gitignore` covers `.env`).

---

## Stage 5 — Glossary subsystem (`glossary`) `[x]`

- [x] `relevant_terms` — find glossary terms present in a chapter (ordered by frequency)
- [x] `build_extraction_prompt` + `parse_extracted_terms` — extract new names/terms
      (JSON, tolerant of code fences); module stays pure, the call is orchestrated
- [x] `merge` — conflict resolution (canon/pinned win, frequency accumulates)
- [x] `TermKind` label/from_label helpers
- [x] 5 unit tests
- [ ] Wire phases 1 & 2 into the per-chapter flow — done in Stage 6 (orchestration)

**Verified** with a real two-phase run: a chapter mentioning `王林` was translated
with the injected canonical "Ван Линь"; extraction then returned `王林` (person)
and `南凰洲` (location) as JSON, and `merge` kept the pinned canon (freq 9→10) and
appended the new location.

---

## Stage 5A — FB2 input parser (`book::fb2`) `[x]`

- [x] Parse FB2 (FictionBook XML) via `quick-xml`: metadata + `<section>/<title>/<p>`
- [x] Language-agnostic heading-number extraction (`第N章`, `Глава N.M`, `Chapter N`, `N.M`)
- [x] Merge split parts (1.1, 1.2 …) into whole chapters; drop front matter
- [x] 3 unit tests

**Verified** on the real reference FB2 `За гранью времени`: 589 sections → 513
chapters (1–514), metadata read, parts merged, content aligns with the source.

---

## Stage 5B — Universal input & chapter detection `[x]`

- [x] `book::load` — detect format (TXT vs FB2) by content → chapters + meta + report
- [x] Generalize TXT chapter detection: `candidate_patterns` (Chinese 第N章/回/话,
      English `Chapter N`, Russian `Глава N`), `detect_chapter_pattern` picks the
      best by match count; `parse_chapters_with` splits on any pattern
- [x] Model-inferred delimiter: `build_delimiter_prompt` + `parse_inferred_pattern`
      (pure); orchestrator does the call. `needs_delimiter` flags unknown layouts
- [ ] Config: input/output format + source/target language — folded into Stage 8 (UI/settings)

**Verified:** all 3 real sample books load through one entry point (976/1350/1662
chapters, GBK auto-detected); a non-standard `=== N ===` layout was split correctly
via a DeepSeek-inferred regex. 21 book-module tests pass.

---

## Stage 5C — Reference translation (`reference`) `[x]`

- [x] `load_reference` — parse a supplied reference via `book::load` (any format)
- [x] `align` — pair source↔reference chapters by number, positional fallback
- [x] `bootstrap_glossary` — DeepSeek extraction of `source → professional
      rendering` over a sample of pairs, marked **pinned** canon
- [x] `style_exemplar` — a professional excerpt for a few-shot style reference
- [x] 3 unit tests (alignment); real bootstrap verified
- [ ] Injection into the translation prompt (style exemplar) — wired in Stage 6

**Verified:** bootstrapping 3 chapters of `За гранью времени` yielded canonical
lore — the whole cultivation system (凝气→Конденсация Ци, 筑基→Возведение Основания,
结丹→Формирование Ядра, 元婴→Зарождение Души) plus names/locations, all pinned,
matching the reference's own glossary. Extraction has some noise (a term can map to
a neighboring phrase); frequency + a 30-chapter sample + manual pinning mitigate it.

---

## Stage 6 — Orchestration (sequential + rolling context) `[x]`

- [x] `Orchestrator` — sequential loop over `pending_chapters` in order (resumable)
- [x] Rolling context: inject running summary + previous chapter tail + style
      exemplar; after each chapter update the summary and persist `meta.running_summary`
- [x] Per chapter: relevant terms → translate → save → update summary →
      extract+merge glossary (enrichment best-effort)
- [x] Failure isolation (mark `failed`, keep going)
- [x] `PromptContext` (glossary + style + summary + prev tail); `build_summary_prompt`
- [ ] Optional parallel mode / progress `emit` — wired at the UI layer (Stage 8)

**Verified (user acceptance test):** translated 5 sequential chapters of the real
book with a reference-bootstrapped glossary + style, then compared to the
professional translation `За гранью времени`:
- **Names consistent and matching the pro**: protagonist "Сюй Цин" identical across
  all 5 chapters and equal to the professional rendering; `南凰洲` → "континент
  Южного Феникса" throughout.
- **Meaning preserved**; chapter 1 nearly matched the professional prose in tone.
- **Rolling summary** produced a coherent running synopsis (plot, characters, lore),
  confirming cross-chapter context carries forward.

---

## Stage 7 — Export + "continue translation" (`export`) `[x]`

- [x] `txt` — concatenated chapters with titles, correct order
- [x] `fb2` — per-chapter `<section>` with titles + book metadata (XML-escaped)
- [x] `OutputFormat` + `export()` dispatch; `TranslatedChapter` carries `number`
- [x] **Continue mode**: `reference::continue_from` (chapters past the reference's
      coverage), `export::combine` (existing translation + new chapters, ordered
      by number), so output = existing pro translation + freshly translated chapters
- [x] **Chapter limit**: `Orchestrator::run(limit)` — translate the next N chapters
- [x] Chapter titles translated too (title prepended to the request; `split_title_body`
      splits + cleans markdown noise), so appended chapters get target-language titles
- [ ] `epub` output — deferred (after TXT/FB2)

**Verified (continue flow):** with a reference covering chapters 1–2, `continue_from`
picked 3–4, the orchestrator translated them (limit 2), `combine` produced a 4-chapter
book, exported to FB2 and re-parsed to 4 chapters. Appended chapters got Russian
titles matching the reference style («Глава 3. Покойтесь… с миром», «Глава 4:
Незваный гость») with consistent names (Сюй Цин).

---

## Stage 8 — UI (Tauri + React) `[x]`

- [x] `commands.rs` bridge with managed `AppState` (session, cancel flag); the run
      executes on a dedicated thread + current-thread runtime (keeps non-`Sync`
      SQLite off the async executor) and streams `progress` / `done` / `job_error`
- [x] Commands: `load_source`, `load_reference`, `bootstrap_glossary`,
      `use_reference_as_base` (continue mode), `start_translation(limit)`,
      `pause_translation`, `get_progress`, `get_glossary`, `update_term`, `export_book`
- [x] React UI: source/reference pickers, sample + chapter-limit inputs, continue
      toggle, Start/Pause, progress bar, live log, editable/pinnable glossary table
- [x] SVG icon master → RGBA raster icons; frontend + Tauri crate both compile
- [ ] Original ↔ translation preview per chapter — later polish

**Verified:** frontend builds (`npm run build`); the Tauri crate `cargo check`s
clean with webkit2gtk installed. Visual launch (`npm run tauri dev`) runs on the
user's desktop (this environment is headless).

**Robustness fix (this stage):** term extraction (bootstrap + per-chapter) now
retries on malformed JSON via `translator::extract_terms` before skipping — a
single bad reply no longer aborts a run (network/HTTP is already retried in the
client).

---

## Stage 9 — Full run & polish `[ ]`

- [ ] End-to-end run of all ~990 chapters of `光阴之外`
- [ ] Review consistency, fix glossary edge cases
- [ ] Cost/time report; document actual numbers
- [ ] Error-path hardening (rate limits, long chapters, resume after crash)

**Verify:** a complete, consistent Russian `.epub`/`.txt` of the book.

---

## Extensions (post-v1, optional)

- Previous-chapter summary injected into the prompt for narrative continuity
- EPUB/PDF/HTML input formats
- Two-pass translation (draft → editorial polish) for higher quality
- Diff/QA view to spot-check machine translation
- Extend the reference bootstrap beyond the initial sample
