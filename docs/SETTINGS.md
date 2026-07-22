# book-converter — Settings matrix

> Where configuration lives. Last updated 2026-07-22.

Settings are intentionally split by lifetime and scope. There is no single
settings object — use this matrix when adding a new knob.

## Overview

| Layer | Storage | Scope | Survives restart | Survives project switch |
|-------|---------|-------|:----------------:|:-----------------------:|
| Environment / `.env` | Process env | App | via shell/dotenv | yes |
| Settings DB | `<app_data>/settings.db` | App-wide | yes | yes |
| localStorage | Browser/WebView | This machine UI | yes | yes |
| Project meta | `projects/<id>/progress.db` `meta` | One project | yes | per project |
| React ephemeral | Memory | Session UI | no | no |

`<app_data>` is `$XDG_DATA_HOME/book-converter` or `~/.local/share/book-converter`
(see `paths::app_data_dir`).

## Environment / `.env`

Loaded by `Config::load()` via `dotenvy` (real env wins over `.env`).

| Variable | Default | Purpose |
|----------|---------|---------|
| `DEEPSEEK_API_KEY` | — | Required for API calls |
| `DEEPSEEK_MODEL` | `deepseek-chat` | Model id |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com` | API base |
| `SOURCE_LANG` | (see settings DB / default Chinese) | Overrides source language |
| `TARGET_LANG` | (see settings DB / default Russian) | Overrides target language |

`Config` also holds `temperature`, `request_timeout_secs`, `max_chunk_chars`,
`max_retries` (code defaults; not currently exposed in the UI).

## Settings DB (`settings.db`)

Key-value table via `settings::get` / `settings::set`. IPC: `get_setting` /
`set_setting`.

| Key | Purpose |
|-----|---------|
| `lang` | UI language (`ru` / `en` / `zh`) |
| `source_lang` | Translation source language name for prompts |
| `target_lang` | Translation target language name for prompts |

`Config::load()` reads `source_lang` / `target_lang` from this DB, then applies
env overrides.

## localStorage (frontend)

| Key | Purpose |
|-----|---------|
| `bc.projects.v2` | Project list (`id`, `path`, `name`, `refPath?`) |
| `bc.active.v2` | Active project index |
| `bc.lang` (`LS_LANG`) | Instant UI-language cache (DB is durable source of truth) |

## Per-project meta (`progress.db`)

Written by load/open/commands; restored on `open_project`.

| Key | Purpose |
|-----|---------|
| `title` / `author` | From source |
| `title_translated` / `author_translated` | Display / export |
| `summary` | Annotation |
| `cover_ct` / `cover_b64` | Cover image |
| `format` / `encoding` | Source format info |
| `running_summary` | Rolling story synopsis for sequential translation (book-level mirror) |

Plus tables `chapters` and `glossary` (not KV meta). Each translated chapter also
stores its own `rolling_summary` and `prev_tail` — the continuity context after
that chapter — so a later single-chapter translate or a resumed run can restore
context without relying only on in-memory state.

## Ephemeral UI state (not persisted)

Chapter limit, bootstrap sample size, re-translate-from index, pane visibility,
glossary highlight toggle, console visibility, collapsed panels, pending glossary
renames (until retarget runs).

## Adding a new setting

1. **App-wide durable preference** → settings DB key + Settings UI.
2. **Secret / deploy override** → env var in `Config::load()`.
3. **Per-book data** → project `meta` (or a dedicated table).
4. **UI chrome only** → React state; localStorage only if it should survive reload.
