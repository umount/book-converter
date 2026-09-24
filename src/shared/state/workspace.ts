import type { BookChapterView, ChapterSummary, ProjectDescriptor, ProjectSettingsView } from "../contracts/generated";
import type { createProjectApi } from "../api/transport";
type Api = Pick<ReturnType<typeof createProjectApi>, "open" | "settings" | "chapters" | "chapter">;
export interface WorkspaceState {
  project: ProjectDescriptor | null;
  settings: ProjectSettingsView | null;
  chapters: ChapterSummary[];
  chapter: BookChapterView | null;
  loading: boolean;
  error: unknown;
}
const empty = (): WorkspaceState => ({ project: null, settings: null, chapters: [], chapter: null, loading: false, error: null });

/** Stable IDs and request generations prevent cross-project/chapter response races. */
export class WorkspaceStore {
  private state: WorkspaceState = empty();
  private generation = 0;
  private chapterGeneration = 0;
  private listeners = new Set<() => void>();
  constructor(private api: Api) {}
  snapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(state: WorkspaceState) { this.state = state; for (const listener of this.listeners) listener(); }
  async open(projectId: string) {
    const generation = ++this.generation;
    ++this.chapterGeneration;
    this.publish({ ...empty(), loading: true });
    try {
      const [project, settings] = await Promise.all([this.api.open({ projectId }), this.api.settings({ projectId })]);
      if (generation !== this.generation) return;
      if (project.id !== projectId) throw new Error("Project response identity mismatch");
      const chapters: ChapterSummary[] = [];
      if (project.kind === "book") {
        let cursor: string | null = null;
        const seen = new Set<string>();
        do {
          const page = await this.api.chapters({ projectId, cursor, limit: 500 });
          if (generation !== this.generation) return;
          chapters.push(...page.items); cursor = page.nextCursor;
          if (cursor !== null && seen.has(cursor)) throw new Error("Repeated chapter cursor");
          if (cursor !== null) seen.add(cursor);
        } while (cursor !== null);
      }
      this.publish({ project, settings, chapters, chapter: null, loading: false, error: null });
      if (chapters[0]) await this.selectChapter(chapters[0].id);
    } catch (error) {
      if (generation === this.generation) this.publish({ ...empty(), error });
    }
  }
  async selectChapter(chapterId: string) {
    const projectId = this.state.project?.id, generation = this.generation, chapterGeneration = ++this.chapterGeneration;
    if (!projectId || !this.state.chapters.some(c => c.id === chapterId)) return;
    this.publish({ ...this.state, chapter: null, loading: true, error: null });
    try {
      const chapter = await this.api.chapter({ projectId, chapterId });
      if (generation !== this.generation || chapterGeneration !== this.chapterGeneration) return;
      if (chapter.chapter.id !== chapterId || chapter.blocks.some(b => b.chapterId !== chapterId)) throw new Error("Chapter response identity mismatch");
      this.publish({ ...this.state, chapter, loading: false });
    } catch (error) {
      if (generation === this.generation && chapterGeneration === this.chapterGeneration) this.publish({ ...this.state, chapter: null, loading: false, error });
    }
  }
  clearError() { this.publish({ ...this.state, error: null }); }
  close() { ++this.generation; ++this.chapterGeneration; this.publish(empty()); }
  dispose() { this.close(); this.listeners.clear(); }
}
