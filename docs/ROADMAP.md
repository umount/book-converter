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

## Stage 2 — Book parsing (`book::parser`) `[ ]`

- [ ] Normalize line endings (CRLF/CR → LF)
- [ ] Regex for chapter markers `第[一二三四五六七八九十百千零两]+章`
- [ ] Build `Chapter { index, title, body }` between markers
- [ ] Parse header metadata (title `《光阴之外》`, author `作者：耳根`, chapter count)
- [ ] Unit tests on a sample slice of the real book

**Verify:** parsing `光阴之外.txt` yields ~990 chapters; first title is `第一章 活着`.

---

## Stage 3 — State store (`state`) `[ ]`

- [ ] Open/create SQLite, apply schema (`chapters`, `glossary`, `meta`)
- [ ] `init_chapters` — idempotent insert (never clobbers existing translations)
- [ ] `pending_chapters`, `save_translation`, `set_status`
- [ ] Reset stray `in_progress` → `pending` on startup
- [ ] Glossary load/save (upsert)

**Verify:** parse → init → kill process → reopen: `pending_chapters` reflects what was left.

---

## Stage 4 — DeepSeek client + prompts (`translator`) `[ ]`

- [ ] `config::load` — key from env `DEEPSEEK_API_KEY`, rest from `config.local.toml`
- [ ] `deepseek::translate` — `POST /chat/completions`, parse response
- [ ] Retry with exponential backoff on 429/5xx/timeouts
- [ ] `prompt::system_prompt` / `user_prompt` (dictionary + text)
- [ ] Cost/size estimation helper (char count × pricing)

**Verify:** translate one real chapter end-to-end from a CLI/test harness; inspect output quality.

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
