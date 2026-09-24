# Settings

Updated 2026-09-24. This describes the active implementation.

## Project settings

A project's kind and language pair are fixed at creation. `project.db` stores role
choices for book translation, assistant and manga processing. Settings updates check
an expected revision; changing processing settings invalidates dependent results.
Book metadata, cover and book/chapter instructions have dedicated service methods.

The overview owns book instructions and batch size. A batch has an explicit positive
chapter limit. Updating glossary terminology does not automatically start correction:
the user receives a modal with candidate counts and confirms a bounded job.

## Providers and credentials

`application/profiles.rs` manages named provider profiles in the app settings database.
Profiles include endpoint, model, generation limits, timeout and retry settings.
Project role settings select profiles. A job captures its resolved provider configuration;
resume rejects an incompatible changed endpoint/profile instead of sending data elsewhere.

The default book provider uses `config.rs`: built-in defaults, saved app settings and
`DEEPSEEK_MODEL`/`DEEPSEEK_BASE_URL` environment overrides. The saved default key takes
precedence over `DEEPSEEK_API_KEY`; `.env` is loaded with dotenvy. Named profiles use
their own saved credentials. Settings responses expose presence/masked hints, not keys.
Credentials are local settings data and are not included in portable project archives.

The settings UI edits the default endpoint/model/key and named role profiles. Interface
language is a UI preference, independent of the project's translation languages.
Global language settings from the removed backend do not control project languages.

## Manga models

The model manager supports download, pause, verification and removal. Model artifacts
are separate from project archives; download URLs/hashes belong to the model manifest.
Automatic manga processing remains pending and must pass capability preflight before
starting. Required weights should be downloaded on demand rather than bundled by default.

See [architecture](ARCHITECTURE.md), [manga requirements](MANGA.md) and
[verification status](REFACTORING_STATUS.md).
