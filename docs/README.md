# Documentation

Start with the [project overview](../README.md) for capabilities and setup.
These documents describe the current implementation, not a development roadmap.

- [Architecture](ARCHITECTURE.md): component boundaries, data ownership, IPC,
  background execution and interaction diagrams.
- [Books](BOOKS.md): importing, translating, reviewing, terminology and export.
- [Narration](NARRATION.md): local Qwen3-TTS, model preparation, MP3s and recovery.
- [Settings](SETTINGS.md): languages, provider roles, credentials and data locations.
- [Book assistant](ASSISTANT.md): context, proposals and confirmation.
- [Development](DEVELOPMENT.md): setup, contracts, tests and packaging.

Keep behavior descriptions with their owning feature. Keep cross-component rules
and diagrams in Architecture, and reproducible development commands in Development.
Changes to IPC must update generated contracts; changes to workflows should update
the corresponding document in the same change.
