# book-converter — Documentation

> Single source of truth for the design of book-converter.
> Last updated 2026-07-17

## Navigation

| Document | Description |
|---|---|
| **[ARCHITECTURE.md](ARCHITECTURE.md)** | System architecture: components, translation pipeline, glossary subsystem, state storage, IPC commands |
| **[ROADMAP.md](ROADMAP.md)** | Staged work plan from scaffold to a finished translated book |
| **[DECISIONS.md](DECISIONS.md)** | Key design decisions and their rationale |
| **[../README.md](../README.md)** | Prerequisites, quickstart, project layout |

## Quick Reference

- **Goal:** translate `光阴之外` (Chinese, ~990 chapters) → Russian, output TXT + EPUB.
- **Provider:** DeepSeek API (`deepseek-chat`), OpenAI-compatible `/chat/completions`.
- **Chunking:** by chapter (`第N章`), fallback to paragraph-boundary splitting.
- **Consistency:** auto-growing glossary injected into every prompt.
- **Resumability:** per-chapter status + translations in SQLite `progress.db`.
- **Stack:** Tauri v2 + React/TS frontend, Rust core (`book_converter_lib`).
