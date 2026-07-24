import type { ChapterRow, Progress, Project } from "../types";
import { ChapterTree } from "./shell/ChapterTree";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  sidebar: boolean;
  projects: Project[];
  active: number;
  setActive: (i: number) => void;
  progressById: Record<string, Progress>;
  busyById: Record<string, string | null>;
  chapters: ChapterRow[];
  activeChapterIdx: number | null;
  onOpenChapter: (idx: number) => void;
  error: string | null;
  setError: (e: string | null) => void;
  onRemove: (idx: number) => void;
  onOpenBook: () => void;
};

/** Explorer: open projects, plus the active project's chapter tree. */
export function Sidebar({
  t, sidebar, projects, active, setActive, progressById, busyById,
  chapters, activeChapterIdx, onOpenChapter, error, setError, onRemove, onOpenBook,
}: Props) {
  if (!sidebar) return null;
  const hasActive = active >= 0 && !!projects[active];
  return (
    <aside className="sidebar">
      <div className="sidebar-head"><span>{t("sidebar.projects")}</span></div>
      <ul className="tree projects">
        {projects.map((p, i) => {
          const running = !!progressById[p.id]?.running;
          const busy = !!busyById[p.id];
          return (
            <li key={p.id}>
              <div className={`node ${i === active ? "active" : ""}`} onClick={() => setActive(i)} title={busyById[p.id] || p.path}>
                <span className={`dot ${running || busy ? "running" : ""}`} />
                <span className="pname">{p.name}</span>
                {(running || busy) && <span className="spinner tiny" />}
                <button className="icon remove" onClick={(e) => { e.stopPropagation(); onRemove(i); }}>×</button>
              </div>
            </li>
          );
        })}
        {projects.length === 0 && <li className="empty">{t("sidebar.noProjects")}</li>}
      </ul>

      {hasActive && (
        <div className="explorer-chapters">
          <div className="sidebar-subhead">{t("explorer.chapters")}</div>
          <ChapterTree t={t} chapters={chapters} activeIdx={activeChapterIdx} onOpen={onOpenChapter} />
        </div>
      )}

      <button className="add" onClick={onOpenBook}>{t("sidebar.openBook")}</button>
      {error && (
        <div className="notif" role="alert">
          <span className="notif-icon">⚠</span>
          <span className="notif-msg">{error}</span>
          <button className="icon" onClick={() => setError(null)}>×</button>
        </div>
      )}
    </aside>
  );
}
