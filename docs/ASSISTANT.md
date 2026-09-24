# Book assistant

Updated 2026-09-24. Implemented by `application/assistant.rs` and
`features/assistant/Assistant.tsx`.

The assistant lives in the right panel. Enter sends; Shift+Enter inserts a newline.
Closing the panel preserves the draft/history while the component remains mounted.
Project messages persist in SQLite. Unconfirmed proposals are in memory, expire after
30 minutes and do not survive application restart.

## Supported actions

- Set complete book instructions.
- Set instructions for the current chapter.
- Add/update a glossary term.
- Preview a literal replacement in the current chapter.
- Start a translation batch with an explicitly requested positive chapter count.

Proposals are prepared by the backend and bound to a project and captured revisions.
Approval calls the same services as editor actions. A stale proposal cannot replace
newer manual work. The UI can apply supported proposals automatically for the current
request only when its auto-apply option is enabled. It does not give the model arbitrary
IPC access. Project language/provider changes and whole-book unbounded translation
are not assistant actions.

## Context and limits

The request includes book instructions, selected chapter context capped at 24,000
characters, up to 20 recent messages capped at 4,000 characters each, and a glossary
sample capped at 100 entries. This is bounded context, not a whole-book search/tool loop.
The model is told to explain missing context rather than claim it has read everything.
This glossary sample differs from translation/correction prompts, which filter terms
locally against the supplied text.

Responses use a constrained JSON schema and at most four proposals. Book text and
history are treated as data. Cancellation aborts the pending turn; active-turn guards
prevent concurrent assistant mutations for the same project. No real-provider quality
or full legacy assistant tool parity is claimed by offline tests.

See [architecture](ARCHITECTURE.md) and [execution status](REFACTORING_STATUS.md).
