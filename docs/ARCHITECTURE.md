# Architecture

Book Converter is a local desktop application. React renders the workspace inside
Tauri; Rust owns project data, file access, provider requests and background jobs.
Project lifecycle, settings, glossary, assets and execution infrastructure serve book workflows.

## Component boundaries

```mermaid
flowchart TB
    subgraph Frontend[React webview]
        UI[AppShell and feature components]
        State[WorkspaceStore / EditorStore / JobStore]
        API[Typed project API]
        UI <--> State
        UI --> API
        State --> API
    end
    subgraph Backend[Rust application]
        IPC[Tauri command adapters]
        Context[AppContext]
        Services[Book / Assistant services]
        Manager[ProjectManager and leases]
        Jobs[Durable job runner]
        Storage[Repositories and transactions]
        Assets[AssetStore and bookasset protocol]
        AI[ChatCompletions transport]
        IPC --> Context
        IPC --> Services
        Services --> Manager
        Services --> Jobs
        Services --> Storage
        Jobs --> Services
        Manager --> Storage
        Services --> Assets
        Services --> AI
    end
    API <-->|Typed IPC| IPC
    Jobs -->|project-event| State
    Storage --> DB[(Project SQLite)]
    Assets --> Files[Project image files]
    Assets -->|Image URLs| UI
    AI --> Provider[Configured external API]
```

`AppContext` is the composition root. It owns `ProjectManager`, job admission and
cancellation registries, edit previews, `AssistantService`.
Commands adapt IPC arguments and responses; domain behavior lives in application
services.

Code entry points:

- [AppShell.tsx](../src/app/AppShell.tsx) composes project navigation, workspaces,
  supporting panels and Jobs. [features](../src/features/) contains feature UI.
- [shared/state](../src/shared/state/) handles workspace response ordering, editor
  saves and job subscriptions.
- [transport.ts](../src/shared/api/transport.ts) defines the typed frontend API;
  [desktop.ts](../src/shared/api/desktop.ts) binds it to Tauri or development fixtures.
- [lib.rs](../src-tauri/src/lib.rs) registers commands, plugins, resources and startup
  recovery. [commands](../src-tauri/src/commands/) is the IPC boundary.
- [application](../src-tauri/src/application/) implements domain workflows.
  [project](../src-tauri/src/project/) owns import, manifests, archives and leases.
- [storage](../src-tauri/src/storage/) owns SQL, result revisions and durable records.
  [assets.rs](../src-tauri/src/assets.rs) serves images; [assets/store.rs](../src-tauri/src/assets/store.rs)
  publishes immutable content-addressed files.
- [ai](../src-tauri/src/ai/) handles provider requests.
- [book](../src-tauri/src/book/) parses book formats and [export](../src-tauri/src/export/)
  writes output formats.

## Contracts and frontend state

Rust DTOs in [app/contracts.rs](../src-tauri/src/app/contracts.rs) and
[app/requests.rs](../src-tauri/src/app/requests.rs) generate
[generated.ts](../src/shared/contracts/generated.ts). Revisions cross IPC as decimal
strings, avoiding JavaScript integer precision loss. Requests identify projects and
entities with stable IDs; mutations carry expected revisions.

Errors retain `code`, `messageKey`, `params` and `retryable` across IPC.
[strings.ts](../src/app/strings.ts) turns them into localized messages, including
provider HTTP errors, malformed glossary responses and language failures.

`WorkspaceStore` ignores late responses after project/chapter selection changes.
`EditorStore` coordinates save/discard operations. `JobStore` subscribes before its
initial job listing, reads updated jobs on events, and rejects older revisions.
It does not continuously poll unchanged jobs. UI preferences and selected locations
can live in local storage; persisted project content remains authoritative in SQLite.

Shared modal windows keep their header/footer outside the scrolling body. Jobs uses
one fixed header/progress area and a scrolling list with active work first.

## Persistent data

The data root is resolved by [paths.rs](../src-tauri/src/paths.rs):
`$XDG_DATA_HOME/book-converter`, otherwise `$HOME/.local/share/book-converter`;
without either environment variable it falls back to a temporary directory.

```text
book-converter/
  settings.db              Application settings, profiles and credentials
  models/                  Downloaded model artifacts and download state
  staging/                 Temporary imports and archive snapshots
  projects/<project-id>/
    project.json           Versioned manifest and project identity
    project.db             Domain data, revisions, jobs and history
    assets/                Images addressed by content hashes
```

The manifest format and database schema use version 1. Unsupported manifests/schema
versions are rejected. Project kind and source/target languages are fixed at creation.
Imported text and registered images are self-contained; reading does not depend on
keeping the original source archive at its old location.

```mermaid
flowchart LR
    Project[Project settings and identity]
    Project --> Chapters[Book chapters]
    Chapters --> Blocks[Ordered text / caption / image blocks]
    Chapters --> Translations[Translation revisions]
    Translations --> Texts[Translated blocks keyed by source ID]
    Translations --> Continuity[Rolling context]
    Blocks --> Assets[(Immutable assets)]
    Project --> Shared[Glossary / settings / assistant history]
    Project --> Runs[Job runs and settings snapshots]
    Runs --> Steps[Attempts / fingerprints / output references]
```

See [schema.sql](../src-tauri/src/storage/schema.sql) and
[schema_extensions.sql](../src-tauri/src/storage/schema_extensions.sql) for actual
constraints. Connections enable foreign keys, WAL and a busy timeout. Leases keep
projects alive while work runs; deletion seals the project against new leases,
cancels work and waits for current users before removing its directory.

## Import, images and portable archives

Import first normalizes a source into a staging project and returns a preview.
Creation assigns project choices and publishes the staged directory. Archive paths,
file counts/sizes and image dimensions are validated; folder import rejects symlinks.
Book block IDs preserve separate occurrences even when image bytes
share one asset hash.

Images are loaded through `bookasset` URLs rather than serialized as large IPC
responses. The backend resolves project identity and asset paths inside the project's
asset directory. Changed images get new asset IDs, so cached URLs cannot silently
show different bytes.

`.bcproj` export uses a SQLite snapshot plus its referenced assets and manifest.
Import validates this portable representation before publication. Credentials and
model weights belong to application settings/runtime storage and are not archived.
Book format export uses a consistent snapshot and an explicit incomplete-book policy.
Output publication refuses to overwrite an existing destination.

## Background execution

A run freezes entity selection, processing settings, provider configuration and
instructions. A step records its entity, stage, attempt, input fingerprint and output
reference. The runner is independent of the webview and works with `StepExecutor`
implementations for books.

```mermaid
sequenceDiagram
    actor User
    participant UI as React workspace
    participant Command as Tauri command
    participant Runner as Durable runner
    participant Service as Stage executor
    participant DB as Project SQLite
    participant Engine as Provider or native worker
    User->>UI: Start a chapter/page batch
    UI->>Command: Typed request with project and selection
    Command->>DB: Save run and frozen settings
    Command-->>UI: JobRef
    Command->>Runner: Dispatch background work
    loop Selected entities and stages
        Runner->>DB: Read checkpoint and record attempt
        Runner->>Service: Compute using current inputs
        Service->>Engine: Text/vision request or local operation
        Engine-->>Service: Stage output
        Service-->>Runner: Candidate result
        Runner->>DB: Recheck dependencies
        alt Inputs are still current
            Runner->>DB: Commit result and successful step together
            Runner-->>UI: project-event with job identity/revision
            UI->>Command: Read updated job and relevant views
        else Inputs changed or computation failed
            Runner->>DB: Record structured failure
            Runner-->>UI: Job update
        end
    end
```

Resume skips successful steps only when their fingerprints and domain validity checks
still match. Changing relevant inputs makes affected work run again. Startup marks
unfinished runs as interrupted; it does not automatically submit new provider calls.
Cancellation preserves already committed steps. Duration measurements feed ETA;
errors are saved with the failed job/attempt.

The shared HTTP transport calls an OpenAI-compatible `/chat/completions` endpoint,
with configured timeout and bounded network retries. Requests are non-streaming.
HTTP 429 and server errors are retryable; domain output validation is handled by the
calling service. Credentials are resolved locally and excluded from run snapshots;
resume checks the saved provider against the configured credential destination.

## Book processing

```mermaid
flowchart LR
    Select[Select eligible chapters in source order] --> Next[Next chapter]
    Next --> Glossary[Optional glossary extraction]
    Glossary --> Translate[Translate title and stable text segments]
    Translate --> Repair[Repair affected foreign-language lines]
    Repair --> Save[Publish translation revision]
    Save --> Context[Update rolling story context]
    Context --> More{More selected chapters?}
    More -->|Yes| Next
    More -->|No| Done[Complete job]
```

Images retain their source positions while translated text is keyed to stable source
block IDs. Translation prompts include relevant terminology, book/chapter instructions
and preceding continuity. Missing/duplicated segment results get bounded repair
attempts. Reference imports supply full translations only for aligned chapters that
can accept them without replacing existing authored work.

Glossary extraction stores returned terms without term-count caps; repeated source
terms share one glossary entry. A term without a literal source match can be kept
with zero frequency. Existing targets, including pinned choices, are retained during
additive extraction. Context payload selection is separate from stored glossary size.

Metadata generation translates title/author/annotation into the project's target
language. Supported-script checks trigger language repair and reject unresolved
foreign-script output. These checks are not universal language detection. Manual
presentation overrides remain separate from generated metadata.

## Assistant actions

```mermaid
sequenceDiagram
    actor User
    participant Panel as Assistant panel
    participant Assistant as AssistantService
    participant API as Configured provider
    participant Services as Book application services
    User->>Panel: Send message about current chapter
    Panel->>Assistant: Message and project/chapter IDs
    Assistant->>API: Bounded context and supported action schema
    API-->>Assistant: Reply and proposed actions
    Assistant-->>Panel: Reply and revision-bound previews
    User->>Panel: Confirm proposal or enable auto-apply
    Panel->>Assistant: Apply proposal
    Assistant->>Services: Invoke ordinary guarded operation
    Services-->>Panel: Saved result or background JobRef
```

The assistant has a finite action set and uses the same services as direct UI edits.
History is persisted; pending proposals live in memory and expire. It cannot bypass
revision checks or change a project's fixed languages. See [Assistant](ASSISTANT.md).

## Invariants for changes

Keep source images immutable, distinguish status from result freshness and review,
and validate captured revisions before publishing asynchronous results. Keep API keys
out of project archives, job snapshots and frontend responses. Starting processing is
an explicit action; viewing, capability inspection and selecting a page do not submit
paid work. Commands validate project identity before processing.

[Development](DEVELOPMENT.md) describes checks for these boundaries. Automated fixtures
verify application behavior; provider quality, font coverage and native packaging
must also be checked on the relevant content and target platform.
