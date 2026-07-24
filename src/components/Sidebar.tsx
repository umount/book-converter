import type { Progress, Project } from "../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  sidebar: boolean;
  projects: Project[];
  active: number;
  setActive: (i: number) => void;
  progressById: Record<string, Progress>;
  busyById: Record<string, string | null>;
  error: string | null;
  setError: (e: string | null) => void;
  onRemove: (idx: number) => void;
  onOpenBook: () => void;
};

/** Explorer: the list of open projects. View navigation lives in the tab bar. */
export function Sidebar({
  t, sidebar, projects, active, setActive,
  progressById, busyById, error, setError, onRemove, onOpenBook,
}: Props) {
  if (!sidebar) return null;
  return (
    <aside className="sidebar">
      <div className="sidebar-head"><span>{t("sidebar.projects")}</span></div>
      <ul className="tree">
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
