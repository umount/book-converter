# book-converter — Documentation

> Design reference for book-converter.
> Last updated 2026-07-20

## Navigation

| Document | Description |
|---|---|
| **[ARCHITECTURE.md](ARCHITECTURE.md)** | System architecture: components, translation pipeline, glossary subsystem, reference translation, state storage, IPC commands |
| **[DECISIONS.md](DECISIONS.md)** | Key design decisions and their rationale |
| **[../README.md](../README.md)** | Product overview, requirements, setup, usage |

## At a glance

- **Goal:** translate book-length works between languages, keeping names, lore,
  and style consistent across the whole book.
- **Provider:** DeepSeek API (`deepseek-chat`), OpenAI-compatible `/chat/completions`.
- **Pipeline:** decode → parse chapters → sequential translation with a rolling
  summary + glossary → export.
- **Consistency:** an auto-growing glossary, optionally bootstrapped (pinned) from
  a professional reference translation, plus a style exemplar.
- **Continuation:** continue a professional translation from where it ends.
- **Resumability:** per-chapter status + translations in a per-book SQLite database.
- **Formats:** TXT / FB2 / ZIP in; FB2 / EPUB / TXT (zipped) out.
- **Stack:** Tauri v2 + React/TS desktop app, Rust core (`book_converter_lib`).
