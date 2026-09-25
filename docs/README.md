# Documentation

Start with the [project overview](../README.md) for capabilities and setup.
These documents describe the current implementation, not a development roadmap.

- [Architecture](ARCHITECTURE.md): component boundaries, data ownership, IPC,
  background execution and interaction diagrams.
- [Books](BOOKS.md): importing, translating, reviewing, terminology and export.
- [Manga](MANGA.md): page processing, region editing and supported output.
- [Settings](SETTINGS.md): languages, provider roles, credentials and data locations.
- [Book assistant](ASSISTANT.md): context, proposals and confirmation.
- [Native manga runtime](MANGA_RUNTIME.md): worker, models, resources and diagnostics.
- [Development](DEVELOPMENT.md): setup, contracts, tests and packaging.

Keep behavior descriptions with their owning feature. Keep cross-component rules
and diagrams in Architecture, and reproducible development commands in Development.
Changes to IPC must update generated contracts; changes to workflows should update
the corresponding document in the same change.
