# Settings and providers

## Application settings and project choices

Interface language is independent of translation language. The default target language
is used when creating projects; changing that default does not change existing projects.
A project's kind and source/target languages are fixed at creation.

Project settings select provider roles: book translation, manga recognition, manga
translation and assistant. Updates use expected revisions. A running job retains its
captured settings; changes apply to new jobs and can make dependent results stale.
Book/chapter instructions, presentation fields and cover have dedicated operations.

## Provider profiles

Settings configures the default endpoint/model/key and named profiles. A named profile
contains its own endpoint, model, temperature, output-token limit, timeout and network
retry count. Profiles use an OpenAI-compatible chat-completions transport; endpoint
configuration should name the API base, not a full `/chat/completions` request URL.

Roles without a named profile use shared defaults. For manga recognition with the
shared DeepSeek endpoint, the current adapter selects `deepseek-flash`. This is an
application routing rule, not a guarantee that a particular account or endpoint accepts
image input. An explicit recognition profile can select another vision-capable model.

Jobs store provider configuration without the credential. Resume compares the captured
profile and endpoint with the currently configured credential destination before using
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
contents. `.bcproj` is the portable project format; model installations are separate.

## Batch sizes and glossary counts

Chapter/page batch sizes bound how much processing a start action requests. They are
not limits on the size of a stored book, project or glossary. Standalone glossary
extraction also uses a chapter batch size; repeat extraction includes already processed
chapters. Returned glossary terms are not truncated to a fixed count.

Changing terminology does not automatically submit correction requests. The user sees
a correction preview and confirms the requested chapter batch.

## Manga models

Settings supports downloading, pausing, verifying and removing model artifacts.
The fixed catalog records repository, revision, size and SHA-256. Interrupted downloads
can be resumed; only verified completed artifacts are available to local processing.
Weights are stored under the application `models/` directory and are not bundled in
project archives. See [Native runtime](MANGA_RUNTIME.md) for the required components.


## Diagnostics

**Full diagnostic logging** applies immediately and persists across restarts.
It records runtime-file validation, processing stages, provider HTTP status/timings,
worker errors and frontend command timings. It does not log provider request/response
bodies, API keys or images. Paths and project/job identifiers can appear in logs.

**Save diagnostic logs** writes a ZIP containing build metadata and recent session
logs. Logs live in the application data directory under `logs/`; the exact path is
shown in Settings. Files rotate at 20 MiB, startup prunes old sessions and export
includes at most five files. Warnings/errors and startup identification remain
available when detailed logging is disabled. Diagnostic builds enable detailed
logging by default, unless a preference was previously saved.
