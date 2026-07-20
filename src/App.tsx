import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

// --- DTOs (mirror src-tauri/src/commands.rs) ---
type BookInfo = {
  title: string;
  author: string;
  total_chapters: number;
  format: string;
  encoding: string;
  needs_delimiter: boolean;
  missing: number;
  duplicates: number;
};
type RefInfo = { title: string; chapters: number; max_covered: number | null };
type Progress = { done: number; total: number; failed: number; pending: number; running: boolean };
type Term = { source: string; target: string; kind: string; frequency: number; pinned: boolean };
type BookDetails = {
  title: string;
  author: string;
  title_translated: string | null;
  summary: string | null;
  cover: string | null;
};

// A project = an opened book (with an optional reference).
type Project = { path: string; name: string; refPath?: string };

const LS_PROJECTS = "bc.projects";
const LS_ACTIVE = "bc.active";

function baseName(p: string): string {
  return p.split(/[\\/]/).pop() || p;
}

export default function App() {
  const [projects, setProjects] = useState<Project[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(LS_PROJECTS) || "[]");
    } catch {
      return [];
    }
  });
  const [active, setActive] = useState<number>(() => Number(localStorage.getItem(LS_ACTIVE) ?? -1));

  const [book, setBook] = useState<BookInfo | null>(null);
  const [ref, setRef] = useState<RefInfo | null>(null);
  const [details, setDetails] = useState<BookDetails | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [glossaryQuery, setGlossaryQuery] = useState("");

  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const [sample, setSample] = useState(30);
  const [limit, setLimit] = useState<number | "">("");
  const [continueMode, setContinueMode] = useState(false);

  const [sidebar, setSidebar] = useState(true);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (k: string) => setCollapsed((c) => ({ ...c, [k]: !c[k] }));

  const [log, setLog] = useState<string[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const addLog = (m: string) => setLog((l) => [...l.slice(-300), m]);

  const activeProject = active >= 0 ? projects[active] : undefined;

  // persist projects
  useEffect(() => localStorage.setItem(LS_PROJECTS, JSON.stringify(projects)), [projects]);
  useEffect(() => localStorage.setItem(LS_ACTIVE, String(active)), [active]);

  // live progress events
  useEffect(() => {
    const unsubs = [
      listen<Progress>("progress", (e) => setProgress(e.payload)),
      listen("done", () => {
        addLog("✓ run finished");
        refreshProgress();
        refreshGlossary();
      }),
      listen<string>("job_error", (e) => setError(String(e.payload))),
    ];
    return () => unsubs.forEach((u) => u.then((f) => f()));
  }, []);

  useEffect(() => logRef.current?.scrollTo(0, logRef.current.scrollHeight), [log]);

  // activate the current project on mount / when it changes
  useEffect(() => {
    if (activeProject) void activateProject(activeProject);
    else clearWorkspace();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  async function call<T>(name: string, args?: Record<string, unknown>): Promise<T | undefined> {
    setError(null);
    try {
      return await invoke<T>(name, args);
    } catch (e) {
      setError(String(e));
      return undefined;
    }
  }

  function clearWorkspace() {
    setBook(null);
    setRef(null);
    setDetails(null);
    setProgress(null);
    setGlossary([]);
  }

  async function activateProject(p: Project) {
    setBusy(`Opening ${p.name}…`);
    clearWorkspace();
    const info = await call<BookInfo>("load_source", { path: p.path });
    if (info) {
      setBook(info);
      if (p.refPath) {
        const r = await call<RefInfo>("load_reference", { path: p.refPath });
        if (r) setRef(r);
      }
      await refreshDetails();
      await refreshProgress();
      await refreshGlossary();
      void translateTitle();
    }
    setBusy(null);
  }

  async function openBook() {
    const path = await open({ filters: [{ name: "Book", extensions: ["txt", "fb2", "zip"] }] });
    if (typeof path !== "string") return;
    const existing = projects.findIndex((p) => p.path === path);
    if (existing >= 0) {
      setActive(existing);
      return;
    }
    const next = [...projects, { path, name: baseName(path) }];
    setProjects(next);
    setActive(next.length - 1);
    addLog(`Opened project: ${baseName(path)}`);
  }

  function removeProject(idx: number) {
    const next = projects.filter((_, i) => i !== idx);
    setProjects(next);
    setActive(idx === active ? -1 : active > idx ? active - 1 : active);
  }

  async function openReference() {
    if (!activeProject) return;
    const path = await open({ filters: [{ name: "Reference", extensions: ["fb2", "txt", "zip"] }] });
    if (typeof path !== "string") return;
    setBusy("Loading reference…");
    const info = await call<RefInfo>("load_reference", { path });
    setBusy(null);
    if (info) {
      setRef(info);
      setProjects((ps) => ps.map((p, i) => (i === active ? { ...p, refPath: path } : p)));
      addLog(`Reference: ${info.chapters} chapters, covers up to #${info.max_covered ?? "?"}`);
      refreshDetails();
    }
  }

  async function bootstrap() {
    setBusy(`Bootstrapping from ${sample} chapters…`);
    const n = await call<number>("bootstrap_glossary", { sample });
    setBusy(null);
    if (n !== undefined) {
      addLog(`Bootstrapped ${n} pinned terms`);
      refreshGlossary();
    }
  }

  async function start() {
    if (continueMode && ref) {
      setBusy("Importing reference chapters…");
      const n = await call<number>("use_reference_as_base", {});
      setBusy(null);
      if (n !== undefined) addLog(`Imported ${n} reference chapters`);
    }
    const lim = limit === "" ? null : Number(limit);
    await call("start_translation", { limit: lim });
    addLog(`Started translation${lim ? ` (next ${lim})` : ""}`);
  }

  async function pause() {
    await call("pause_translation", {});
    addLog("Pause requested");
  }

  async function refreshProgress() {
    const p = await call<Progress>("get_progress", {});
    if (p) setProgress(p);
  }
  async function refreshGlossary() {
    const g = await call<Term[]>("get_glossary", {});
    if (g) setGlossary(g);
  }
  async function refreshDetails() {
    const d = await call<BookDetails>("get_book_details", {});
    if (d) setDetails(d);
  }
  async function translateTitle() {
    const t = await call<string>("translate_title", {});
    if (t) refreshDetails();
  }
  async function replaceCover() {
    const path = await open({ filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "gif", "webp"] }] });
    if (typeof path !== "string") return;
    await call("set_cover", { path });
    refreshDetails();
  }
  async function saveSummary(text: string) {
    await call("set_summary", { summary: text });
  }
  async function pinTerm(t: Term, target: string) {
    await call("update_term", { term: { ...t, target, pinned: true } });
    refreshGlossary();
  }
  async function exportBook() {
    const outPath = await save({ filters: [{ name: "Output", extensions: ["fb2", "epub", "txt", "zip"] }] });
    if (typeof outPath !== "string") return;
    setBusy("Exporting…");
    const p = await call<string>("export_book", { outPath });
    setBusy(null);
    if (p) addLog(`Exported → ${p}`);
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  const filteredGlossary = useMemo(() => {
    const q = glossaryQuery.trim().toLowerCase();
    if (!q) return glossary;
    return glossary.filter((t) => t.source.toLowerCase().includes(q) || t.target.toLowerCase().includes(q));
  }, [glossary, glossaryQuery]);

  const Panel = ({ id, title, extra, children }: { id: string; title: string; extra?: ReactNode; children: ReactNode }) => (
    <section className="panel">
      <div className="panel-head" onClick={() => toggle(id)}>
        <span className={`chevron ${collapsed[id] ? "closed" : ""}`}>▾</span>
        <span className="panel-title">{title}</span>
        <span className="panel-extra" onClick={(e) => e.stopPropagation()}>{extra}</span>
      </div>
      {!collapsed[id] && <div className="panel-body">{children}</div>}
    </section>
  );

  return (
    <div className="ide">
      {/* Title / menu bar */}
      <header className="titlebar">
        <div className="brand">book-converter</div>
        <nav className="menu">
          <button onClick={openBook}>Open book</button>
          <button onClick={openReference} disabled={!activeProject}>Reference</button>
          <button onClick={exportBook} disabled={!progress || progress.done === 0}>Export</button>
          <div className="spacer" />
          {busy && <span className="busy-inline">{busy}</span>}
        </nav>
      </header>

      <div className="body">
        {/* Sidebar: projects */}
        <aside className={`sidebar ${sidebar ? "" : "collapsed"}`}>
          <div className="sidebar-head">
            <span>Projects</span>
            <button className="icon" title="Collapse" onClick={() => setSidebar(false)}>⟨</button>
          </div>
          <ul className="projects">
            {projects.map((p, i) => (
              <li key={p.path} className={i === active ? "active" : ""} onClick={() => setActive(i)} title={p.path}>
                <span className="dot" />
                <span className="pname">{p.name}</span>
                <button className="icon remove" title="Remove" onClick={(e) => { e.stopPropagation(); removeProject(i); }}>×</button>
              </li>
            ))}
            {projects.length === 0 && <li className="empty">No projects</li>}
          </ul>
          <button className="add" onClick={openBook}>+ Open book</button>
        </aside>

        {!sidebar && (
          <button className="sidebar-show" title="Show projects" onClick={() => setSidebar(true)}>⟩</button>
        )}

        {/* Work area */}
        <main className="workarea">
          {error && <div className="error" onClick={() => setError(null)}>{error}</div>}

          {!activeProject ? (
            <div className="welcome">
              <h1>book-converter</h1>
              <p>Open a book to start a project.</p>
              <button onClick={openBook}>Open book</button>
            </div>
          ) : (
            <>
              <div className="workhead">
                <div className="worktitle">{details?.title_translated || details?.title || activeProject.name}</div>
                <div className="worksub">
                  {book && `${book.total_chapters} ch · ${book.format} · ${book.encoding}`}
                  {ref && ` · ref covers #${ref.max_covered ?? "?"}`}
                </div>
              </div>

              {/* Book details */}
              <Panel id="book" title="Book">
                <div className="book-details">
                  {details?.cover ? (
                    <img className="cover" src={details.cover} alt="cover" />
                  ) : (
                    <div className="cover cover-empty">no cover</div>
                  )}
                  <div className="book-meta">
                    <div className="row">
                      <strong className="book-title">{details?.title_translated || details?.title || "(untitled)"}</strong>
                      {details && !details.title_translated && <button onClick={translateTitle}>Translate title</button>}
                      <button onClick={replaceCover}>Replace cover</button>
                    </div>
                    {details?.title_translated && details?.title && <div className="muted">original: {details.title}</div>}
                    <div className="muted">{details?.author}</div>
                    <textarea
                      className="summary"
                      placeholder="Summary / annotation…"
                      key={active + (details?.summary ?? "")}
                      defaultValue={details?.summary || ""}
                      onBlur={(e) => saveSummary(e.target.value)}
                    />
                  </div>
                </div>
              </Panel>

              {/* Translate */}
              <Panel id="translate" title="Translate" extra={
                <>
                  <button onClick={openReference}>Reference</button>
                  {ref && (
                    <>
                      <input type="number" min={1} value={sample} onChange={(e) => setSample(Number(e.target.value))} style={{ width: 60 }} title="bootstrap sample" />
                      <button onClick={bootstrap}>Bootstrap</button>
                    </>
                  )}
                </>
              }>
                {ref && (
                  <label className="check">
                    <input type="checkbox" checked={continueMode} onChange={(e) => setContinueMode(e.target.checked)} />
                    Continue mode (keep professional chapters)
                  </label>
                )}
                <div className="row" style={{ marginTop: 8 }}>
                  <label>chapters</label>
                  <input type="number" min={1} placeholder="all" value={limit}
                    onChange={(e) => setLimit(e.target.value === "" ? "" : Number(e.target.value))} style={{ width: 80 }} />
                  <button onClick={start} disabled={progress?.running}>Start</button>
                  <button onClick={pause} disabled={!progress?.running}>Pause</button>
                  <button onClick={refreshProgress} className="ghost">↻</button>
                </div>
                {progress && (
                  <div className="progress">
                    <div className="bar"><div className="bar-fill" style={{ width: `${pct}%` }} /></div>
                    <div className="progress-text">
                      {progress.done}/{progress.total} ({pct}%)
                      {progress.failed > 0 && ` · failed ${progress.failed}`}
                      {progress.running ? " · running" : ""}
                    </div>
                  </div>
                )}
              </Panel>

              {/* Glossary */}
              <Panel id="glossary" title={`Glossary (${glossary.length})`} extra={
                <>
                  <input placeholder="filter…" value={glossaryQuery} onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 120 }} />
                  <button onClick={refreshGlossary}>↻</button>
                </>
              }>
                <div className="table-wrap">
                  <table>
                    <thead><tr><th>Source</th><th>Translation</th><th>Kind</th><th>×</th><th>📌</th></tr></thead>
                    <tbody>
                      {filteredGlossary.map((t) => (
                        <tr key={t.source}>
                          <td>{t.source}</td>
                          <td><input defaultValue={t.target} onBlur={(e) => e.target.value !== t.target && pinTerm(t, e.target.value)} /></td>
                          <td>{t.kind}</td>
                          <td>{t.frequency}</td>
                          <td>{t.pinned ? "📌" : ""}</td>
                        </tr>
                      ))}
                      {filteredGlossary.length === 0 && <tr><td colSpan={5} className="empty">empty</td></tr>}
                    </tbody>
                  </table>
                </div>
              </Panel>

              {/* Log */}
              <Panel id="log" title="Log">
                <div className="log" ref={logRef}>{log.map((l, i) => <div key={i}>{l}</div>)}</div>
              </Panel>
            </>
          )}
        </main>
      </div>
    </div>
  );
}
