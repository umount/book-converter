# Settings and providers

## Application settings and project choices

Interface language is independent of translation language. The default target language
is used when creating projects; changing that default does not change existing projects.
A project's kind and source/target languages are fixed at creation.

Project settings select provider roles: book translation and assistant. Updates use expected revisions. A running job retains its
captured settings; changes apply to new jobs and can make dependent results stale.
Book/chapter instructions, presentation fields and cover have dedicated operations.

## Provider profiles

Settings configures the default endpoint/model/key and named profiles. A named profile
contains its own endpoint, model, temperature, output-token limit, timeout and network
retry count. Profiles use an OpenAI-compatible chat-completions transport; endpoint
configuration should name the API base, not a full `/chat/completions` request URL.

Roles without a named profile use shared defaults. These profiles configure
translation and the book assistant. Local narration uses the bundled Qwen3-TTS
engine and does not require a provider endpoint or API key.

Translation and assistant jobs store provider configuration without the credential.
Resume compares the captured profile and endpoint with the currently configured
credential destination before using
a local key. Changing the endpoint/profile can therefore require starting a new job.

## Credentials and environment

Profiles and credentials are stored in the local application `settings.db`. Keys are
not project content and are not included in `.bcproj` archives. Settings responses expose
presence/masked hints, not credential values. Protect the data directory accordingly;
the implementation uses SQLite storage rather than an OS credential vault.

The default provider is resolved by [config.rs](../src-tauri/src/config.rs):

1. Built-in defaults supply unset values.
2. Saved application settings supply model, base URL, retries and temperature.
3. `DEEPSEEK_MODEL` and `DEEPSEEK_BASE_URL` override the saved model and endpoint.
4. A saved nonempty default key takes precedence over `DEEPSEEK_API_KEY`.

A local `.env` is loaded through dotenvy; already-set environment variables take
precedence over its values. Named profiles use their own saved credentials. Do not
put keys into project instructions, documentation or committed configuration.

The data root is `$XDG_DATA_HOME/book-converter`, falling back to
`$HOME/.local/share/book-converter`. The [architecture](ARCHITECTURE.md) describes its
contents. If neither environment variable exists, the current implementation uses
the system temporary directory as its base. This lookup also applies on Windows;
it does not automatically choose `%APPDATA%`. Set `XDG_DATA_HOME` to a persistent
directory before launching when neither variable is defined.

Shared speech model files are in `tts-models/`; audio inputs, progress, checkpoints
and generated MP3s are in `audiobooks/<project-id>/<job-id>/` under the data root.
`.bcproj` is the portable project format and excludes these narration files. See
[Narration](NARRATION.md) for model preparation and audio export.

## Batch sizes and glossary counts

Chapter batch sizes bound how much processing a start action requests. They are
not limits on the size of a stored book, project or glossary. Standalone glossary
extraction also uses a chapter batch size; repeat extraction includes already processed
chapters. Returned glossary terms are not truncated to a fixed count.

Changing terminology does not automatically submit correction requests. The user sees
a correction preview and confirms the requested chapter batch.


## Diagnostics

**Full diagnostic logging** applies immediately and persists across restarts.
It records processing stages, provider HTTP status/timings and frontend command timings. Rejected translation responses can be written to logs, including translated or source text. API keys are redacted. Paths and project/job identifiers can appear in logs.

**Save diagnostic logs** writes a ZIP containing build metadata and recent session
logs. Logs live in the application data directory under `logs/`; the exact path is
shown in Settings. Files rotate at 20 MiB, startup prunes old sessions and export
includes at most five files. Warnings/errors and startup identification remain
available when detailed logging is disabled. Diagnostic builds enable detailed
logging by default, unless a preference was previously saved.
