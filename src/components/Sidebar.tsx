import type { Progress, Project, ViewId } from "../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  sidebar: boolean;
  setSidebar: (v: boolean) => void;
  projects: Project[];
  active: number;
  setActive: (i: number) => void;
  view: ViewId;
  setView: (v: ViewId) => void;
  progressById: Record<string, Progress>;
  busyById: Record<string, string | null>;
  glossaryCount: number;
  error: string | null;
  setError: (e: string | null) => void;
  onRemove: (idx: number) => void;
  onOpenBook: () => void;
};

export function Sidebar({
  t, sidebar, setSidebar, projects, active, setActive, view, setView,
  progressById, busyById, glossaryCount, error, setError, onRemove, onOpenBook,
}: Props) {
  return (
    <>
      <aside className={`sidebar ${sidebar ? "" : "collapsed"}`}>
        <div className="sidebar-head"><span>{t("sidebar.projects")}</span>
          <button className="icon" onClick={() => setSidebar(false)}>⟨</button>
        </div>
        <ul className="tree">
          {projects.map((p, i) => {
            const running = !!progressById[p.id]?.running;
            const busy = !!busyById[p.id];
            return (
              <li key={p.id}>
                <div className={`node ${i === active ? "active" : ""}`} onClick={() => setActive(i)} title={busyById[p.id] || p.path}>
                  <span className={`dot ${running || busy ? "running" : ""}`} /><span className="pname">{p.name}</span>
                  {(running || busy) && <span className="spinner tiny" />}
                  <button className="icon remove" onClick={(e) => { e.stopPropagation(); onRemove(i); }}>×</button>
                </div>
                {i === active && (
                  <ul className="children">
                    <li className={`leaf ${view === "overview" ? "sel" : ""}`} onClick={() => setView("overview")}>{t("nav.overview")}</li>
                    <li className={`leaf ${view === "reader" ? "sel" : ""}`} onClick={() => setView("reader")}>{t("nav.translation")}</li>
                    <li className={`leaf ${view === "glossary" ? "sel" : ""}`} onClick={() => setView("glossary")}>{t("nav.glossary")} <span className="badge">{glossaryCount}</span></li>
                  </ul>
                )}
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
      {!sidebar && <button className="sidebar-show" onClick={() => setSidebar(true)}>⟩</button>}
    </>
  );
}
