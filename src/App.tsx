import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

// --- DTOs (mirror src-tauri/src/commands.rs) ---
type BookInfo = {
  title: string; author: string; total_chapters: number;
  format: string; encoding: string; needs_delimiter: boolean; missing: number; duplicates: number;
};
type RefInfo = { title: string; chapters: number; max_covered: number | null };
type Progress = { done: number; total: number; failed: number; pending: number; running: boolean };
type Term = { source: string; target: string; kind: string; frequency: number; pinned: boolean };
type BookDetails = { title: string; author: string; title_translated: string | null; author_translated: string | null; summary: string | null; cover: string | null };
type ChapterRow = { idx: number; number: number | null; title: string; status: string };
type ChapterView = {
  idx: number; number: number | null; source_title: string; source: string;
  translated_title: string | null; translated: string | null; status: string;
};

type Project = { path: string; name: string; refPath?: string };
type ViewId = "overview" | "reader" | "glossary";

const LS_PROJECTS = "bc.projects";
const LS_ACTIVE = "bc.active";
const baseName = (p: string) => p.split(/[\\/]/).pop() || p;
const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// Wrap glossary terms present in `text` with <mark>.
function highlight(text: string, terms: string[]): ReactNode[] {
  const present = Array.from(new Set(terms.filter((t) => t && text.includes(t))))
    .sort((a, b) => b.length - a.length)
    .slice(0, 400);
  if (present.length === 0) return [text];
  const re = new RegExp(`(${present.map(escapeRe).join("|")})`, "g");
  return text.split(re).map((part, i) => (i % 2 === 1 ? <mark key={i}>{part}</mark> : part));
}

export default function App() {
  const [projects, setProjects] = useState<Project[]>(() => {
    try { return JSON.parse(localStorage.getItem(LS_PROJECTS) || "[]"); } catch { return []; }
  });
  const [active, setActive] = useState<number>(() => Number(localStorage.getItem(LS_ACTIVE) ?? -1));
  const [view, setView] = useState<ViewId>("overview");

  const [book, setBook] = useState<BookInfo | null>(null);
  const [ref, setRef] = useState<RefInfo | null>(null);
  const [details, setDetails] = useState<BookDetails | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [glossaryQuery, setGlossaryQuery] = useState("");

  const [chapters, setChapters] = useState<ChapterRow[]>([]);
  const [chapterIdx, setChapterIdx] = useState<number | null>(null);
  const [chapter, setChapter] = useState<ChapterView | null>(null);
  const [chapterLoading, setChapterLoading] = useState(false);
  const [panes, setPanes] = useState({ orig: true, transl: true });
  const [hl, setHl] = useState(true);

  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [menu, setMenu] = useState<"file" | "view" | null>(null);

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

  useEffect(() => localStorage.setItem(LS_PROJECTS, JSON.stringify(projects)), [projects]);
  useEffect(() => localStorage.setItem(LS_ACTIVE, String(active)), [active]);
  useEffect(() => logRef.current?.scrollTo(0, logRef.current.scrollHeight), [log]);

  useEffect(() => {
    const unsubs = [
      listen<Progress>("progress", (e) => setProgress(e.payload)),
      listen("done", () => { addLog("✓ run finished"); refreshProgress(); refreshGlossary(); }),
      listen<string>("job_error", (e) => setError(String(e.payload))),
    ];
    return () => unsubs.forEach((u) => u.then((f) => f()));
  }, []);

  useEffect(() => {
    if (activeProject) void activateProject(activeProject);
    else clearWorkspace();
    setView("overview");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  useEffect(() => {
    if (view === "reader") void loadChapters();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view]);

  useEffect(() => {
    if (chapterIdx != null) void openChapter(chapterIdx);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx]);

  async function call<T>(name: string, args?: Record<string, unknown>): Promise<T | undefined> {
    setError(null);
    try { return await invoke<T>(name, args); } catch (e) { setError(String(e)); return undefined; }
  }

  function clearWorkspace() {
    setBook(null); setRef(null); setDetails(null); setProgress(null);
    setGlossary([]); setChapters([]); setChapterIdx(null); setChapter(null);
  }

  async function activateProject(p: Project) {
    setBusy(`Opening ${p.name}…`);
    clearWorkspace();
    const info = await call<BookInfo>("load_source", { path: p.path });
    if (info) {
      setBook(info);
      if (p.refPath) { const r = await call<RefInfo>("load_reference", { path: p.refPath }); if (r) setRef(r); }
      await refreshDetails(); await refreshProgress(); await refreshGlossary();
      void translateTitle();
    }
    setBusy(null);
  }

  async function openBook() {
    setMenu(null);
    const path = await open({ filters: [{ name: "Book", extensions: ["txt", "fb2", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    const existing = projects.findIndex((p) => p.path === path);
    if (existing >= 0) { setActive(existing); return; }
    const next = [...projects, { path, name: baseName(path) }];
    setProjects(next); setActive(next.length - 1);
    addLog(`Opened project: ${baseName(path)}`);
  }

  function removeProject(idx: number) {
    setProjects(projects.filter((_, i) => i !== idx));
    setActive(idx === active ? -1 : active > idx ? active - 1 : active);
  }

  async function openReference() {
    setMenu(null);
    if (!activeProject) return;
    const path = await open({ filters: [{ name: "Reference", extensions: ["fb2", "txt", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    setBusy("Loading reference…");
    const info = await call<RefInfo>("load_reference", { path });
    setBusy(null);
    if (info) {
      setRef(info);
      setProjects((ps) => ps.map((p, i) => (i === active ? { ...p, refPath: path } : p)));
      addLog(`Reference: covers up to #${info.max_covered ?? "?"}`);
      refreshDetails();
    }
  }

  async function bootstrap() {
    setBusy(`Bootstrapping from ${sample} chapters…`);
    const n = await call<number>("bootstrap_glossary", { sample });
    setBusy(null);
    if (n !== undefined) { addLog(`Bootstrapped ${n} terms`); refreshGlossary(); }
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
  async function pause() { await call("pause_translation", {}); addLog("Pause requested"); }

  async function refreshProgress() { const p = await call<Progress>("get_progress", {}); if (p) setProgress(p); }
  async function refreshGlossary() { const g = await call<Term[]>("get_glossary", {}); if (g) setGlossary(g); }
  async function refreshDetails() { const d = await call<BookDetails>("get_book_details", {}); if (d) setDetails(d); }
  async function translateTitle() { const t = await call<string>("translate_title", {}); if (t) refreshDetails(); }
  async function loadChapters() {
    const cs = await call<ChapterRow[]>("list_chapters", {});
    if (cs) {
      setChapters(cs);
      if (chapterIdx == null && cs.length) setChapterIdx((cs.find((c) => c.status === "done") || cs[0]).idx);
    }
  }
  async function openChapter(idx: number) {
    setChapterLoading(true);
    const c = await call<ChapterView>("get_chapter", { index: idx });
    if (c) setChapter(c);
    setChapterLoading(false);
  }

  async function replaceCover() {
    const path = await open({ filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "gif", "webp"] }] });
    if (typeof path !== "string") return;
    await call("set_cover", { path }); refreshDetails();
  }
  async function saveSummary(text: string) { await call("set_summary", { summary: text }); }
  async function pinTerm(t: Term, target: string) { await call("update_term", { term: { ...t, target, pinned: true } }); refreshGlossary(); }
  async function exportAs(fmt: "fb2" | "epub" | "pdf" | "txt") {
    setMenu(null);
    const outPath = await save({ defaultPath: `book.${fmt}`, filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }] });
    if (typeof outPath !== "string") return;
    const path = outPath.toLowerCase().endsWith(`.${fmt}`) ? outPath : `${outPath}.${fmt}`;
    setBusy(`Exporting ${fmt.toUpperCase()}…`);
    const p = await call<string>("export_book", { outPath: path });
    setBusy(null);
    if (p) addLog(`Exported → ${p}`);
  }
  const canExport = !!progress && progress.done > 0;

  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  const filteredGlossary = useMemo(() => {
    const q = glossaryQuery.trim().toLowerCase();
    return q ? glossary.filter((t) => t.source.toLowerCase().includes(q) || t.target.toLowerCase().includes(q)) : glossary;
  }, [glossary, glossaryQuery]);
  const sourceTerms = useMemo(() => glossary.map((t) => t.source), [glossary]);
  const targetTerms = useMemo(() => glossary.map((t) => t.target), [glossary]);

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
      {/* Menu bar (Cursor-like) */}
      <header className="menubar">
        <span className="brand">book-converter</span>
        <div className="menuitem" onClick={() => setMenu(menu === "file" ? null : "file")}>
          File
          {menu === "file" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={openBook}>Open book…</div>
              <div className={`mi ${!activeProject ? "disabled" : ""}`} onClick={() => activeProject && openReference()}>Open reference…</div>
              <div className="sep" />
              <div className="mi-label">Export as</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("fb2")}>FB2</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("epub")}>EPUB</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("pdf")}>PDF</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("txt")}>TXT</div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={() => setMenu(menu === "view" ? null : "view")}>
          View
          {menu === "view" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={() => { setSidebar((s) => !s); setMenu(null); }}>Toggle sidebar</div>
              <div className="mi" onClick={() => { setPanes({ orig: true, transl: true }); setMenu(null); }}>Show both panes</div>
              <div className="mi" onClick={() => { setHl((h) => !h); setMenu(null); }}>Toggle glossary highlight</div>
            </div>
          )}
        </div>
        <div className="menu-spacer" />
        {busy && <span className="busy-inline">{busy}</span>}
      </header>
      {busy && <div className="loadbar" />}
      {menu && <div className="menu-backdrop" onClick={() => setMenu(null)} />}

      <div className="body">
        {/* Sidebar: projects tree */}
        <aside className={`sidebar ${sidebar ? "" : "collapsed"}`}>
          <div className="sidebar-head"><span>Projects</span>
            <button className="icon" onClick={() => setSidebar(false)}>⟨</button>
          </div>
          <ul className="tree">
            {projects.map((p, i) => (
              <li key={p.path}>
                <div className={`node ${i === active ? "active" : ""}`} onClick={() => setActive(i)} title={p.path}>
                  <span className="dot" /><span className="pname">{p.name}</span>
                  <button className="icon remove" onClick={(e) => { e.stopPropagation(); removeProject(i); }}>×</button>
                </div>
                {i === active && (
                  <ul className="children">
                    <li className={`leaf ${view === "overview" ? "sel" : ""}`} onClick={() => setView("overview")}>Overview</li>
                    <li className={`leaf ${view === "reader" ? "sel" : ""}`} onClick={() => setView("reader")}>Translation</li>
                    <li className={`leaf ${view === "glossary" ? "sel" : ""}`} onClick={() => setView("glossary")}>Glossary <span className="badge">{glossary.length}</span></li>
                  </ul>
                )}
              </li>
            ))}
            {projects.length === 0 && <li className="empty">No projects</li>}
          </ul>
          <button className="add" onClick={openBook}>+ Open book</button>
        </aside>
        {!sidebar && <button className="sidebar-show" onClick={() => setSidebar(true)}>⟩</button>}

        {/* Work area */}
        <main className="workarea">
          {error && <div className="error" onClick={() => setError(null)}>{error}</div>}

          {!activeProject ? (
            <div className="welcome"><h1>book-converter</h1><p>Open a book to start a project.</p><button onClick={openBook}>Open book</button></div>
          ) : (
            <>
              <div className="workhead">
                <div className="worktitle">{details?.title_translated || details?.title || activeProject.name}</div>
                <div className="worksub">
                  {book && `${book.total_chapters} ch · ${book.format} · ${book.encoding}`}
                  {ref && ` · ref #${ref.max_covered ?? "?"}`}
                  {progress && ` · ${progress.done}/${progress.total} done`}
                </div>
              </div>

              {view === "overview" && (
                <>
                  <Panel id="book" title="Book">
                    <div className="book-details">
                      {details?.cover ? <img className="cover" src={details.cover} alt="cover" /> : <div className="cover cover-empty">no cover</div>}
                      <div className="book-meta">
                        <div className="row">
                          <strong className="book-title">{details?.title_translated || details?.title || "(untitled)"}</strong>
                          {details && !details.title_translated && <button onClick={translateTitle}>Translate title</button>}
                          <button onClick={replaceCover}>Replace cover</button>
                        </div>
                        {details?.title_translated && details?.title && <div className="muted">original: {details.title}</div>}
                        <div className="muted">{details?.author_translated || details?.author}</div>
                        <textarea className="summary" placeholder="Summary / annotation…"
                          key={active + (details?.summary ?? "")} defaultValue={details?.summary || ""}
                          onBlur={(e) => saveSummary(e.target.value)} />
                      </div>
                    </div>
                  </Panel>

                  <Panel id="translate" title="Translate" extra={
                    <>
                      <button onClick={openReference}>Reference</button>
                      {ref && <>
                        <input type="number" min={1} value={sample} onChange={(e) => setSample(Number(e.target.value))} style={{ width: 56 }} title="sample" />
                        <button onClick={bootstrap}>Bootstrap</button>
                      </>}
                    </>
                  }>
                    {ref && <label className="check"><input type="checkbox" checked={continueMode} onChange={(e) => setContinueMode(e.target.checked)} /> Continue mode</label>}
                    <div className="row" style={{ marginTop: 8 }}>
                      <label>chapters</label>
                      <input type="number" min={1} placeholder="all" value={limit} onChange={(e) => setLimit(e.target.value === "" ? "" : Number(e.target.value))} style={{ width: 80 }} />
                      <button onClick={start} disabled={progress?.running}>Start</button>
                      <button onClick={pause} disabled={!progress?.running}>Pause</button>
                      <button className="ghost" onClick={refreshProgress}>↻</button>
                    </div>
                    {progress && (
                      <div className="progress">
                        <div className="bar"><div className="bar-fill" style={{ width: `${pct}%` }} /></div>
                        <div className="progress-text">{progress.done}/{progress.total} ({pct}%){progress.failed > 0 && ` · failed ${progress.failed}`}{progress.running ? " · running" : ""}</div>
                      </div>
                    )}
                  </Panel>
                </>
              )}

              {view === "glossary" && (
                <Panel id="glossary" title={`Glossary (${glossary.length})`} extra={
                  <>
                    <input placeholder="filter…" value={glossaryQuery} onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 140 }} />
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
                            <td>{t.kind}</td><td>{t.frequency}</td><td>{t.pinned ? "📌" : ""}</td>
                          </tr>
                        ))}
                        {filteredGlossary.length === 0 && <tr><td colSpan={5} className="empty">empty</td></tr>}
                      </tbody>
                    </table>
                  </div>
                </Panel>
              )}

              {view === "reader" && (
                <div className="reader">
                  <div className="reader-toolbar">
                    <button className="ghost" disabled={!chapters.length} onClick={() => {
                      const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i > 0) setChapterIdx(chapters[i - 1].idx);
                    }}>‹</button>
                    <select value={chapterIdx ?? ""} onChange={(e) => setChapterIdx(Number(e.target.value))}>
                      {chapters.map((c) => (
                        <option key={c.idx} value={c.idx}>
                          {c.status === "done" ? "✓ " : "· "}{c.title.slice(0, 60)}
                        </option>
                      ))}
                    </select>
                    <button className="ghost" disabled={!chapters.length} onClick={() => {
                      const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i >= 0 && i < chapters.length - 1) setChapterIdx(chapters[i + 1].idx);
                    }}>›</button>
                    <div className="menu-spacer" />
                    <label className="check"><input type="checkbox" checked={hl} onChange={(e) => setHl(e.target.checked)} /> highlight</label>
                    <button className={`chip ${panes.orig ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, orig: !p.orig }))}>Original</button>
                    <button className={`chip ${panes.transl ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, transl: !p.transl }))}>Translation</button>
                  </div>

                  <div className="panes">
                    {panes.orig && (
                      <div className="pane">
                        <div className="pane-head">
                          <span>Original {chapter?.number != null && `· #${chapter.number}`}</span>
                          <button className="icon" onClick={() => setPanes((p) => ({ ...p, orig: false }))}>×</button>
                        </div>
                        <div className="pane-body">
                          {chapterLoading ? <div className="loading"><span className="spinner" /> loading…</div> : <>
                            <div className="chtitle">{chapter?.source_title}</div>
                            <div className="chtext">{chapter ? (hl ? highlight(chapter.source, sourceTerms) : chapter.source) : ""}</div>
                          </>}
                        </div>
                      </div>
                    )}
                    {panes.transl && (
                      <div className="pane">
                        <div className="pane-head">
                          <span>Translation {chapter?.status !== "done" && "· not translated"}</span>
                          <button className="icon" onClick={() => setPanes((p) => ({ ...p, transl: false }))}>×</button>
                        </div>
                        <div className="pane-body">
                          {chapterLoading ? <div className="loading"><span className="spinner" /> loading…</div> : <>
                            <div className="chtitle">{chapter?.translated_title}</div>
                            <div className="chtext">
                              {chapter?.translated ? (hl ? highlight(chapter.translated, targetTerms) : chapter.translated) : <span className="muted">— not translated yet —</span>}
                            </div>
                          </>}
                        </div>
                      </div>
                    )}
                    {!panes.orig && !panes.transl && <div className="muted" style={{ padding: 20 }}>Both panes closed. Use the toggles above.</div>}
                  </div>
                </div>
              )}
            </>
          )}
        </main>
      </div>
    </div>
  );
}
