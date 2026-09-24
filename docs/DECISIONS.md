# Current design decisions

Updated 2026-09-24. Implementation evidence is in [REFACTORING_STATUS.md](REFACTORING_STATUS.md).

1. **Books, backend cleanup, then manga.** Complete the book workflow before expanding
   automatic manga processing. Native acceptance remains a separate gate.
2. **Explicit project identity.** Book and Manga share lifecycle/assets/jobs but have
   separate domain tables and workspaces. Project kind and languages are immutable.
3. **Structural content.** Stable blocks/regions carry text and image identity. Literal
   marker-looking text is never interpreted as an instruction to insert an image.
4. **Bounded paid work.** Translation and glossary correction use explicit batch limits.
   Glossary edits only offer correction; confirmation starts the job. Applicable terms
   are matched locally before translation/correction requests.
5. **Preserve authored work.** A reference supplies complete translations of aligned
   untranslated chapters. Origins and error/review flags remain distinct. Revisions
   and transactional checks prevent background output from overwriting newer edits.
6. **Durable execution.** Persist stages, input fingerprints and output references;
   resume completed work without replaying it. Temporary output is never a completed result.
7. **Partial corrections.** A dedicated prompt receives existing translation fragments,
   original context and matching glossary entries. Unaffected text is preserved.
8. **Shared assistant services.** Assistant proposals use editor/application services
   with the same guards. The assistant has a finite supported action set.
9. **Self-contained projects.** Source imports and immutable assets belong to the project.
   Archives contain consistent database snapshots; credentials and model weights stay outside.
10. **Explicit evidence.** Fake providers and browser fixtures establish behavior, not
    translation/OCR quality or cross-platform native acceptance. No repeated legacy reset.
