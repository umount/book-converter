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

- **Goal:** a **universal** book translator (configurable language pair; TXT/FB2
  in, TXT/FB2 out, EPUB planned). Primary run: `光阴之外` / "За гранью времени"
  (~1354 chapters) Chinese → Russian.
- **Provider:** DeepSeek API (`deepseek-chat`), OpenAI-compatible `/chat/completions`.
- **Chunking:** by chapter, fallback to paragraph-boundary splitting.
- **Chapter detection:** generic patterns; model-inferred delimiter for unknown layouts.
- **Consistency:** auto-growing glossary, optionally bootstrapped (pinned) from a
  professional reference translation + style exemplar.
- **Resumability:** per-chapter status + translations in SQLite `progress.db`.
- **Stack:** Tauri v2 + React/TS frontend, Rust core (`book_converter_lib`).
