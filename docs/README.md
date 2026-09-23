# book-converter — Documentation

> Design reference for book-converter.
> Last updated 2026-09-23

## Navigation

| Document | Description |
|---|---|
| **[ARCHITECTURE.md](ARCHITECTURE.md)** | System architecture: components, translation pipeline, glossary, reference, state, IPC |
| **[DECISIONS.md](DECISIONS.md)** | Key design decisions and their rationale |
| **[PROJECT_ISOLATION.md](PROJECT_ISOLATION.md)** | Per-project data dirs, parallel runs, `.bcproj` |
| **[SETTINGS.md](SETTINGS.md)** | Where configuration lives (env, settings DB, localStorage, project meta) |
| **[FRONTEND_REDESIGN.md](FRONTEND_REDESIGN.md)** | Planned Cursor-style IDE frontend rework: shell, editor, highlight, find/replace, settings |
| **[ASSISTANT.md](ASSISTANT.md)** | Living plan: right-side project assistant chat (agent + tools + confirm) |
| **[REFACTORING.md](REFACTORING.md)** | Full breaking-refactor execution plan: contracts, schema, P00–P13, acceptance and AI handoff |
| **[MANGA_TOOLING.md](MANGA_TOOLING.md)** | Verified tooling assessment, lightweight model budgets and Windows/macOS/Linux release gates |
| **[MANGA.md](MANGA.md)** | Architecture proposal: Book/Manga project types, separate workspaces, manga pipeline and staged migration |
| **[../README.md](../README.md)** | Product overview, requirements, setup, usage |

## At a glance

- **Goal:** translate book-length works between languages, keeping names, lore,
  and style consistent across the whole book.
- **Provider:** DeepSeek API (`deepseek-chat`), OpenAI-compatible `/chat/completions`.
- **Pipeline:** decode → parse chapters → sequential translation with a rolling
  summary + glossary (long chapters split on paragraphs) → export.
- **Consistency:** an auto-growing glossary, optionally bootstrapped (pinned) from
  a professional reference translation, plus a style exemplar.
- **Continuation:** continue a professional translation from where it ends.
- **Resumability:** per-chapter status + translations in a per-project SQLite database.
- **Formats:** TXT / FB2 / PDF / ZIP in; TXT / FB2 / EPUB / PDF out (zipped optional).
- **Stack:** Tauri v2 + React/TS desktop app, Rust core (`book_converter_lib`).
