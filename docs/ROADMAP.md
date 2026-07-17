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

## Stage 5 — Glossary subsystem (`glossary`) `[ ]`

- [ ] `relevant_terms` — find glossary terms present in a chapter
- [ ] `extract_terms` — second light request to pull new names/terms + category
- [ ] `merge` — conflict resolution (canon/pinned win, frequency bumps)
- [ ] Wire phases 1 & 2 into the per-chapter flow

**Verify:** translate 3–4 sequential chapters; the protagonist's name stays identical; glossary grows.

---

## Stage 6 — Orchestration (worker pool) `[ ]`

- [ ] Bounded-concurrency pool (`concurrency` in-flight requests)
- [ ] Resumable queue from `pending_chapters`
- [ ] Failure isolation (mark `failed`, keep going), re-run of `failed`
- [ ] Progress aggregation + `emit("progress", …)`

**Verify:** run 20 chapters with concurrency=4; interrupt mid-way; resume finishes only the remainder.

---

## Stage 7 — Export (`export`) `[ ]`

- [ ] `txt` — concatenated chapters with headers, correct order
- [ ] `epub` — per-chapter sections + TOC via `epub-builder`
- [ ] Export command returns file paths; open via `tauri-plugin-opener`

**Verify:** open the produced `.epub` in a reader; TOC lists all chapters.

---

## Stage 8 — UI (Tauri + React) `[ ]`

- [ ] Book picker (`tauri-plugin-dialog`) → summary
- [ ] Start/Pause + progress bar wired to the `progress` event
- [ ] Glossary table: view, edit, pin a term
- [ ] Original ↔ translation preview per chapter
- [ ] Export controls; settings (API key, concurrency, model)

**Verify:** translate the full book from the UI, edit a term mid-run, export.

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
- Other input formats (EPUB/PDF/HTML)
- Other language pairs (config-driven)
- Two-pass translation (draft → editorial polish) for higher quality
- Diff/QA view to spot-check machine translation
