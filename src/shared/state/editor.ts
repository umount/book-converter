import type { BookChapterView } from "../contracts/generated";
import type { createProjectApi } from "../api/transport";
type Api = Pick<ReturnType<typeof createProjectApi>, "chapter" | "editTranslation">;
export interface EditorState { view: BookChapterView; drafts: ReadonlyMap<string, string>; saving: boolean; error: unknown }
/** Serialize whole-translation revisions while retaining edits typed during a save. */
export class BookEditorSession {
  private state: EditorState;
  private listeners = new Set<() => void>();
  private generation = 0;
  private pending: Promise<void> | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  constructor(private api: Api, readonly projectId: string, view: BookChapterView) {
    this.state = { view, drafts: new Map(), saving: false, error: null };
  }
  snapshot = () => this.state;
  subscribe = (fn: () => void) => { this.listeners.add(fn); return () => { this.listeners.delete(fn); }; };
  private publish(patch: Partial<EditorState>) { this.state = { ...this.state, ...patch }; for (const fn of this.listeners) fn(); }
  edit(id: string, text: string) {
    if (!this.state.view.translation || !this.state.view.blocks.some(b => b.id === id && b.content.kind !== "image")) return;
    ++this.generation;
    const drafts = new Map(this.state.drafts); drafts.set(id, text); this.publish({ drafts });
    clearTimeout(this.timer);
    // A conflict remains visible until the user explicitly retries or reloads.
    if (!this.state.error) this.timer = setTimeout(() => { void this.flush().catch(() => {}); }, 650);
  }
  flush(): Promise<void> {
    clearTimeout(this.timer);
    if (this.pending) return this.pending;
    if (!this.state.drafts.size) return Promise.resolve();
    this.publish({ saving: true, error: null });
    this.pending = (async () => {
      while (this.state.drafts.size) {
        const [blockId, text] = this.state.drafts.entries().next().value!;
        const translation = this.state.view.translation;
        if (!translation) throw new Error("Missing translation");
        const revision = await this.api.editTranslation({ projectId: this.projectId, translationId: translation.id, blockId, text, expectedRevision: translation.revision });
        const view = await this.api.chapter({ projectId: this.projectId, chapterId: this.state.view.chapter.id });
        if (view.chapter.id !== this.state.view.chapter.id || view.translation?.revision !== revision) throw { code: "revision_conflict", messageKey: "errors.revisionConflict" };
        const drafts = new Map(this.state.drafts);
        if (drafts.get(blockId) === text) drafts.delete(blockId);
        this.publish({ view, drafts });
      }
    })().catch(error => { this.publish({ error }); throw error; }).finally(() => { this.pending = null; this.publish({ saving: false }); });
    return this.pending;
  }
  async refresh() {
    if (this.state.drafts.size || this.pending) return;
    const generation = this.generation;
    const view = await this.api.chapter({ projectId: this.projectId, chapterId: this.state.view.chapter.id });
    if (generation === this.generation && !this.state.drafts.size && !this.pending && view.chapter.id === this.state.view.chapter.id) this.publish({ view, error: null });
  }
  async discard() {
    ++this.generation;
    if (this.pending) await this.pending.catch(() => {});
    const view = await this.api.chapter({ projectId: this.projectId, chapterId: this.state.view.chapter.id });
    clearTimeout(this.timer); this.publish({ view, drafts: new Map(), error: null });
  }
  dispose() { clearTimeout(this.timer); this.listeners.clear(); }
}
