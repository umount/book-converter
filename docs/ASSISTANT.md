# Book assistant

The book assistant is a right-side panel implemented by
[Assistant.tsx](../src/features/assistant/Assistant.tsx) and
[assistant.rs](../src-tauri/src/application/assistant.rs). Enter sends a message;
Shift+Enter inserts a newline. Hiding the panel preserves its draft while mounted.
Project conversation history is persisted in SQLite.

## Supported actions

- Replace the complete book instructions.
- Set instructions for the current chapter.
- Add or update a glossary term.
- Preview a literal text replacement in the current chapter.
- Start a translation batch with an explicitly requested positive chapter count.

The model returns a reply and structured suggestions. The backend prepares previews
bound to the project and captured revisions. Confirmation invokes the same services
used by editor controls; stale proposals cannot overwrite newer work. The UI's explicit
auto-apply option can apply supported proposals for that request.

Pending proposals live in memory, expire after 30 minutes and do not survive application
restart. Cancelling a turn aborts the pending request. Active-turn guards prevent
concurrent assistant mutations for the same project.

## Context

The request contains book instructions, selected chapter context capped at 24,000
characters, up to 20 recent messages capped at 4,000 characters each, and up to 100
glossary entries. Replies may contain at most four action proposals. These are request
context limits, not limits on the project's stored glossary or conversation history.
Translation/correction services independently select terms matching their supplied text.

The assistant does not have unrestricted IPC access, whole-book browsing or manga
actions. Project language/provider changes and unbounded whole-book translation are
not supported actions. Book text and conversation history are supplied as data, and
the model is asked to acknowledge missing context.

See [Architecture](ARCHITECTURE.md) for the proposal/confirmation sequence and
[Settings](SETTINGS.md) for the assistant provider role.
