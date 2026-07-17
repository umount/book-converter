# book-converter

Desktop app to translate **large** books from Chinese to Russian via the **DeepSeek
API**. Built with a Rust core (Tauri v2) and a React + TypeScript frontend.

Target case: the web novel `光阴之外` — ~11 MB, ~990 chapters.

See [`docs/`](docs/README.md) for architecture, roadmap, and design decisions.

## Prerequisites

- **Rust** (stable) + Cargo — installed (`cargo 1.94`)
- **Node.js** ≥ 18 + npm — installed (`node 26`, `npm 11`)
- **Tauri system libs (Linux)** — currently **missing**, install before building:

  ```bash
  sudo apt update
  sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

  (`webkit2gtk-4.1` is the WebView backend Tauri renders into on Linux.)

- **DeepSeek API key** — put it in a local `.env` (gitignored, auto-loaded at
  startup):

  ```bash
  cp .env.example .env
  # then edit .env and set DEEPSEEK_API_KEY=sk-...
  ```

  Exporting `DEEPSEEK_API_KEY` in the shell also works and takes precedence.

## Quickstart

```bash
npm install                    # frontend deps
cargo install tauri-cli        # or use the npm devDependency: npx tauri
npm run tauri dev              # launch the app in dev mode
```

Build a release bundle:

```bash
npm run tauri build
```

## Project layout

```
book-converter/
├── src/                 # React + TS frontend
├── src-tauri/           # Rust core (book_converter_lib) + Tauri app
│   └── src/
│       ├── book/        # parser + chunker
│       ├── glossary/    # translation-consistency subsystem
│       ├── translator/  # DeepSeek client + prompts
│       ├── state/       # SQLite progress store
│       ├── export/      # TXT + EPUB
│       ├── config.rs
│       └── commands.rs  # Tauri IPC bridge
└── docs/                # ARCHITECTURE, ROADMAP, DECISIONS
```

## Status

Scaffold + design docs. Implementation follows the staged plan in
[`docs/ROADMAP.md`](docs/ROADMAP.md).
