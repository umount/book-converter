import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { LANGS, LS_LANG, normalizeLang, translate, type Lang } from "./i18n";

// --- DTOs (mirror src-tauri/src/commands.rs) ---
type BookInfo = {
  title: string; author: string; total_chapters: number;
  format: string; encoding: string; needs_delimiter: boolean; missing: number; duplicates: number;
};
type RefInfo = { title: string; chapters: number; max_covered: number | null; imported: number };
type Progress = { done: number; total: number; failed: number; pending: number; running: boolean };
type Term = { source: string; target: string; kind: string; frequency: number; pinned: boolean };
const TERM_KINDS = ["person", "location", "organization", "term"] as const;
type BookDetails = { title: string; author: string; title_translated: string | null; author_translated: string | null; summary: string | null; cover: string | null };
type ChapterRow = { idx: number; number: number | null; title: string; status: string; origin: string | null };
type ChapterView = {
  idx: number; number: number | null; source_title: string; source: string;
  translated_title: string | null; translated: string | null; status: string; origin: string | null;
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
  const [lang, setLang] = useState<Lang>(() => normalizeLang(localStorage.getItem(LS_LANG)));
  const [showSettings, setShowSettings] = useState(false);
  const t = (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars);

  const [book, setBook] = useState<BookInfo | null>(null);
  const [ref, setRef] = useState<RefInfo | null>(null);
  const [details, setDetails] = useState<BookDetails | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [glossaryQuery, setGlossaryQuery] = useState("");
  const [newTerm, setNewTerm] = useState<{ source: string; target: string; kind: string }>({ source: "", target: "", kind: "person" });
  // Renames whose new rendering has been saved to the glossary but not yet
  // propagated into the existing translation. Keyed by source; `old` is the
  // rendering still present in the translated text. Drives the global button.
  const [pending, setPending] = useState<Record<string, { old: string; new: string; kind: string }>>({});
  const pendingCount = Object.keys(pending).length;

  const [chapters, setChapters] = useState<ChapterRow[]>([]);
  const [chapterIdx, setChapterIdx] = useState<number | null>(null);
  const chapterIdxRef = useRef<number | null>(null);
  const activatingRef = useRef<string | null>(null);
  const [chapter, setChapter] = useState<ChapterView | null>(null);
  const [chapterLoading, setChapterLoading] = useState(false);
  const [panes, setPanes] = useState({ orig: true, transl: true });
  const [hl, setHl] = useState(true);

  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [menu, setMenu] = useState<"file" | "view" | null>(null);

  const [sample, setSample] = useState(30);
  const [limit, setLimit] = useState<number | "">("");
  const [reFrom, setReFrom] = useState<number>(1);
  const [sidebar, setSidebar] = useState(true);
  const [showConsole, setShowConsole] = useState(true);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (k: string) => setCollapsed((c) => ({ ...c, [k]: !c[k] }));

  const [log, setLog] = useState<string[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const addLog = (m: string) => setLog((l) => [...l.slice(-300), m]);

  const activeProject = active >= 0 ? projects[active] : undefined;

  useEffect(() => localStorage.setItem(LS_PROJECTS, JSON.stringify(projects)), [projects]);
  useEffect(() => localStorage.setItem(LS_ACTIVE, String(active)), [active]);
  // Language: localStorage is an instant cache to avoid a flash on load; the DB
  // is the durable source of truth (survives restarts). Load DB once on mount,
  // then persist every change to both.
  const langLoaded = useRef(false);
  useEffect(() => {
    (async () => {
      const v = await call<string | null>("get_setting", { key: "lang" });
      if (v) setLang(normalizeLang(v));
      langLoaded.current = true;
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    localStorage.setItem(LS_LANG, lang);
    document.documentElement.lang = lang;
    if (langLoaded.current) void call("set_setting", { key: "lang", value: lang });
  }, [lang]);
  useEffect(() => logRef.current?.scrollTo(0, logRef.current.scrollHeight), [log]);

  // Keep a live ref to `t` so once-registered event listeners localize correctly.
  const tRef = useRef(t);
  tRef.current = t;
  useEffect(() => {
    const tr = (key: string, vars?: Record<string, string | number>) => tRef.current(key, vars);
    const unsubs = [
      listen<Progress>("progress", (e) => setProgress(e.payload)),
      listen("done", () => { addLog(tr("log.runFinished")); refreshProgress(); refreshGlossary(); }),
      listen<string>("job_error", (e) => { setBusy(null); reportCritical(String(e.payload)); }),
      listen<{ done: number; total: number; title: string; changed: boolean }>("retarget_progress", (e) => {
        const { done, total, title, changed } = e.payload;
        setBusy(tr("busy.updatingTranslationN", { done, total }));
        addLog(tr("log.retargetItem", { done, total, mark: changed ? "✓" : "·", title }));
      }),
      listen<string>("retarget_warn", (e) => addLog(tr("log.retargetWarn", { msg: String(e.payload) }))),
      listen<number>("retarget_done", (e) => { setBusy(null); addLog(tr("log.renamed", { n: e.payload })); setPending({}); refreshProgress(); const i = chapterIdxRef.current; if (i != null) openChapter(i); }),
    ];
    return () => unsubs.forEach((u) => u.then((f) => f()));
  }, []);

  useEffect(() => {
    if (activeProject) void activateProject(activeProject);
    else { activatingRef.current = null; clearWorkspace(); }
    setView("overview");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  useEffect(() => {
    if (view === "reader") void loadChapters();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view]);

  useEffect(() => {
    chapterIdxRef.current = chapterIdx;
    if (chapterIdx != null) void openChapter(chapterIdx);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx]);

  // Errors always go to the console. The banner (an IDE-style notification) is
  // reserved for critical failures (DeepSeek being unreachable, a project failing
  // to open), so it isn't raised for every minor command hiccup.
  function logError(msg: string) { addLog(tRef.current("log.error", { msg })); }
  function reportCritical(msg: string) { logError(msg); setError(msg); }

  async function call<T>(name: string, args?: Record<string, unknown>, opts?: { critical?: boolean }): Promise<T | undefined> {
    try { return await invoke<T>(name, args); }
    catch (e) { const msg = String(e); logError(msg); if (opts?.critical) setError(msg); return undefined; }
  }

  function clearWorkspace() {
    setBook(null); setRef(null); setDetails(null); setProgress(null);
    setGlossary([]); setChapters([]); setChapterIdx(null); setChapter(null); setPending({});
  }

  async function activateProject(p: Project) {
    // Guard against the double invocation of the `[active]` effect (React
    // StrictMode in dev double-fires effects), which would open the book twice.
    if (activatingRef.current === p.path) return;
    activatingRef.current = p.path;
    setBusy(t("busy.opening", { name: p.name }));
    addLog(t("log.opening", { name: p.name }));
    setError(null);
    clearWorkspace();
    const info = await call<BookInfo>("load_source", { path: p.path }, { critical: true });
    if (info) {
      setBook(info);
      addLog(t("log.loaded", { name: p.name, n: info.total_chapters, format: info.format, encoding: info.encoding }));
      if (p.refPath) { const r = await call<RefInfo>("load_reference", { path: p.refPath }); if (r) { setRef(r); addLog(t("log.reference", { n: r.max_covered ?? "?" })); if (r.imported > 0) addLog(t("log.refImported", { n: r.imported })); } }
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
    setBusy(t("busy.loadingReference"));
    const info = await call<RefInfo>("load_reference", { path });
    setBusy(null);
    if (info) {
      setRef(info);
      setProjects((ps) => ps.map((p, i) => (i === active ? { ...p, refPath: path } : p)));
      addLog(t("log.reference", { n: info.max_covered ?? "?" }));
      if (info.imported > 0) addLog(t("log.refImported", { n: info.imported }));
      refreshDetails(); refreshProgress(); loadChapters();
      const i = chapterIdxRef.current; if (i != null) openChapter(i);
    }
  }

  async function bootstrap() {
    setBusy(t("busy.bootstrapping", { n: sample }));
    const n = await call<number>("bootstrap_glossary", { sample }, { critical: true });
    setBusy(null);
    if (n !== undefined) { addLog(t("log.bootstrapped", { n })); refreshGlossary(); }
  }

  async function start() {
    const lim = limit === "" ? null : Number(limit);
    await call("start_translation", { limit: lim });
    addLog(t("log.started", { suffix: lim ? t("log.startedNext", { n: lim }) : "" }));
  }
  async function pause() { await call("pause_translation", {}); addLog(t("log.pauseRequested")); }
  // Reset chapters to pending for a fresh run with the current glossary. `pos` is a
  // 1-based reading-order position; null means the whole book.
  async function reTranslate(pos: number | null) {
    if (progress?.running) return;
    const total = progress?.total ?? book?.total_chapters ?? 0;
    const msg = pos == null
      ? t("translate.retranslateAllConfirm", { n: total })
      : t("translate.retranslateFromConfirm", { from: pos });
    if (!confirm(msg)) return;
    const fromIndex = pos == null ? null : Math.max(1, pos) - 1;
    const n = await call<number>("reset_translation", { fromIndex });
    if (n !== undefined) { addLog(t("log.reset", { n })); refreshProgress(); }
  }

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
  // Term edits auto-save. Changing the rendering also records a pending rename so
  // the global "Update translation" button can later propagate it into the text.
  async function saveTermField(term: Term, patch: Partial<Term>) {
    await call("update_term", { term: { ...term, ...patch, pinned: true } });
    refreshGlossary();
  }
  function editTarget(term: Term, value: string) {
    const nt = value.trim();
    if (!nt || nt === term.target) return;
    void saveTermField(term, { target: nt });
    setPending((p) => {
      const prev = p[term.source];
      const old = prev ? prev.old : term.target; // rendering still in the translated text
      if (old === nt) { const n = { ...p }; delete n[term.source]; return n; }
      return { ...p, [term.source]: { old, new: nt, kind: prev?.kind ?? term.kind } };
    });
  }
  function editKind(term: Term, kind: string) {
    void saveTermField(term, { kind });
    setPending((p) => (p[term.source] ? { ...p, [term.source]: { ...p[term.source], kind } } : p));
  }
  // Propagate all pending renames into the already-translated text (background job).
  async function updateTranslation() {
    const list = Object.values(pending);
    if (!list.length) return;
    const summary = list.map((c) => `«${c.old}» → «${c.new}»`).join(", ");
    if (!confirm(t("glossary.updateConfirm", { n: list.length, list: list.map((c) => `«${c.old}» → «${c.new}»`).join("\n") }))) return;
    setBusy(t("busy.updatingTranslation"));
    setError(null);
    addLog(t("log.retargetStart", { n: list.length, list: summary }));
    // Progress/finish are driven by retarget_progress / retarget_done events;
    // only a synchronous rejection needs to clear the busy state here.
    try {
      await invoke("retarget_terms", {
        changes: list.map((c) => ({ old_target: c.old, new_target: c.new, kind: c.kind })),
      });
    } catch (e) { setBusy(null); logError(String(e)); }
  }
  async function deleteTerm(t: Term) {
    await call("delete_term", { source: t.source });
    setPending((p) => { const n = { ...p }; delete n[t.source]; return n; });
    refreshGlossary();
  }
  async function renameTerm(t: Term, source: string) {
    await call("update_term", { term: { ...t, source, pinned: true } });
    await call("delete_term", { source: t.source });
    refreshGlossary();
  }
  async function addTerm() {
    const source = newTerm.source.trim(), target = newTerm.target.trim();
    if (!source || !target) return;
    await call("update_term", { term: { source, target, kind: newTerm.kind, frequency: 1, pinned: true } });
    setNewTerm({ source: "", target: "", kind: "person" });
    refreshGlossary();
  }
  async function exportAs(fmt: "fb2" | "epub" | "pdf" | "txt") {
    setMenu(null);
    const outPath = await save({ defaultPath: `book.${fmt}`, filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }] });
    if (typeof outPath !== "string") return;
    const path = outPath.toLowerCase().endsWith(`.${fmt}`) ? outPath : `${outPath}.${fmt}`;
    setBusy(t("busy.exporting", { fmt: fmt.toUpperCase() }));
    const p = await call<string>("export_book", { outPath: path });
    setBusy(null);
    if (p) addLog(t("log.exported", { path: p }));
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
          {t("menu.file")}
          {menu === "file" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={openBook}>{t("file.openBook")}</div>
              <div className={`mi ${!activeProject ? "disabled" : ""}`} onClick={() => activeProject && openReference()}>{t("file.openReference")}</div>
              <div className="sep" />
              <div className="mi-label">{t("file.exportAs")}</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("fb2")}>FB2</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("epub")}>EPUB</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("pdf")}>PDF</div>
              <div className={`mi ${!canExport ? "disabled" : ""}`} onClick={() => canExport && exportAs("txt")}>TXT</div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={() => setMenu(menu === "view" ? null : "view")}>
          {t("menu.view")}
          {menu === "view" && (
            <div className="dropdown" onClick={(e) => e.stopPropagation()}>
              <div className="mi" onClick={() => { setSidebar((s) => !s); setMenu(null); }}>{t("view.toggleSidebar")}</div>
              <div className="mi" onClick={() => { setPanes({ orig: true, transl: true }); setMenu(null); }}>{t("view.showBothPanes")}</div>
              <div className="mi" onClick={() => { setHl((h) => !h); setMenu(null); }}>{t("view.toggleHighlight")}</div>
              <div className="mi" onClick={() => { setShowConsole((s) => !s); setMenu(null); }}>{t("view.toggleConsole")}</div>
            </div>
          )}
        </div>
        <div className="menuitem" onClick={() => { setShowSettings(true); setMenu(null); }}>{t("menu.settings")}</div>
        <div className="menu-spacer" />
        {busy && <span className="busy-inline">{busy}</span>}
      </header>
      {busy && <div className="loadbar" />}
      {menu && <div className="menu-backdrop" onClick={() => setMenu(null)} />}

      <div className="body">
        {/* Sidebar: projects tree */}
        <aside className={`sidebar ${sidebar ? "" : "collapsed"}`}>
          <div className="sidebar-head"><span>{t("sidebar.projects")}</span>
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
                    <li className={`leaf ${view === "overview" ? "sel" : ""}`} onClick={() => setView("overview")}>{t("nav.overview")}</li>
                    <li className={`leaf ${view === "reader" ? "sel" : ""}`} onClick={() => setView("reader")}>{t("nav.translation")}</li>
                    <li className={`leaf ${view === "glossary" ? "sel" : ""}`} onClick={() => setView("glossary")}>{t("nav.glossary")} <span className="badge">{glossary.length}</span></li>
                  </ul>
                )}
              </li>
            ))}
            {projects.length === 0 && <li className="empty">{t("sidebar.noProjects")}</li>}
          </ul>
          <button className="add" onClick={openBook}>{t("sidebar.openBook")}</button>
          {error && (
            <div className="notif" role="alert">
              <span className="notif-icon">⚠</span>
              <span className="notif-msg">{error}</span>
              <button className="icon" onClick={() => setError(null)}>×</button>
            </div>
          )}
        </aside>
        {!sidebar && <button className="sidebar-show" onClick={() => setSidebar(true)}>⟩</button>}

        {/* Right column: work area + console */}
        <div className="rightcol">
        <main className="workarea">
          {showSettings ? (
            <div className="settings-page">
              <div className="workhead settings-head">
                <div className="worktitle">{t("settings.title")}</div>
                <button className="ghost" onClick={() => setShowSettings(false)}>{t("settings.close")}</button>
              </div>
              <Panel id="settings-lang" title={t("settings.language")}>
                <div className="row">
                  <select value={lang} onChange={(e) => setLang(normalizeLang(e.target.value))} style={{ minWidth: 160 }}>
                    {LANGS.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}
                  </select>
                </div>
                <div className="muted" style={{ marginTop: 6 }}>{t("settings.languageHint")}</div>
              </Panel>
            </div>
          ) : (<>
          {!activeProject ? (
            <div className="welcome"><h1>book-converter</h1><p>{t("welcome.subtitle")}</p><button onClick={openBook}>{t("welcome.openBook")}</button></div>
          ) : (
            <>
              <div className="workhead">
                <div className="worktitle">{details?.title_translated || details?.title || activeProject.name}</div>
                <div className="worksub">
                  {book && `${t("overview.chapters", { n: book.total_chapters })} · ${book.format} · ${book.encoding}`}
                  {ref && ` · ${t("overview.ref", { n: ref.max_covered ?? "?" })}`}
                  {progress && ` · ${t("overview.done", { done: progress.done, total: progress.total })}`}
                </div>
              </div>

              {view === "overview" && (
                <>
                  <Panel id="book" title={t("panel.book")}>
                    <div className="book-details">
                      {details?.cover ? <img className="cover" src={details.cover} alt="cover" /> : <div className="cover cover-empty">{t("book.noCover")}</div>}
                      <div className="book-meta">
                        <div className="row">
                          <strong className="book-title">{details?.title_translated || details?.title || t("book.untitled")}</strong>
                          {details && !details.title_translated && <button onClick={translateTitle}>{t("book.translateTitle")}</button>}
                          <button onClick={replaceCover}>{t("book.replaceCover")}</button>
                        </div>
                        {details?.title_translated && details?.title && <div className="muted">{t("book.original", { title: details.title })}</div>}
                        <div className="muted">{details?.author_translated || details?.author}</div>
                        <textarea className="summary" placeholder={t("book.summaryPlaceholder")}
                          key={active + (details?.summary ?? "")} defaultValue={details?.summary || ""}
                          onBlur={(e) => saveSummary(e.target.value)} />
                      </div>
                    </div>
                  </Panel>

                  <Panel id="reference" title={t("panel.reference")}>
                    {!ref ? (
                      <div className="ref-empty">
                        <p className="muted ref-desc">{t("reference.description")}</p>
                        <button onClick={openReference}>{t("reference.add")}</button>
                      </div>
                    ) : (
                      <>
                        <div className="row">
                          <strong>{ref.title || t("book.untitled")}</strong>
                          <span className="muted">{t("reference.covers", { n: ref.max_covered ?? "?", chapters: ref.chapters })}</span>
                          <div className="menu-spacer" />
                          <button className="ghost" onClick={openReference}>{t("reference.replace")}</button>
                        </div>
                        <div className="row" style={{ marginTop: 10 }}>
                          <label>{t("reference.bootstrapLabel")}</label>
                          <input type="number" min={1} value={sample} onChange={(e) => setSample(Number(e.target.value))} style={{ width: 64 }} title={t("translate.sample")} />
                          <button onClick={bootstrap}>{t("translate.bootstrap")}</button>
                        </div>
                      </>
                    )}
                  </Panel>

                  <Panel id="translate" title={t("panel.translate")}>
                    <div className="row">
                      <label>{t("translate.next")}</label>
                      <input type="number" min={1} placeholder={t("translate.allRemaining")} value={limit} onChange={(e) => setLimit(e.target.value === "" ? "" : Number(e.target.value))} style={{ width: 120 }} />
                      <span className="muted">{t("translate.chapters")}</span>
                      <button onClick={start} disabled={progress?.running}>{t("translate.start")}</button>
                      <button onClick={pause} disabled={!progress?.running}>{t("translate.pause")}</button>
                      <button className="ghost" onClick={refreshProgress}>↻</button>
                    </div>
                    {progress && (
                      <div className="muted resume-hint">
                        {progress.pending > 0
                          ? t("translate.resumeHint", { from: progress.done + 1, remaining: progress.pending })
                          : t("translate.resumeHintAllDone")}
                      </div>
                    )}
                    {progress && (
                      <div className="progress">
                        <div className="bar"><div className="bar-fill" style={{ width: `${pct}%` }} /></div>
                        <div className="progress-text">{progress.done}/{progress.total} ({pct}%){progress.failed > 0 && ` · ${t("progress.failed", { n: progress.failed })}`}{progress.running ? ` · ${t("progress.running")}` : ""}</div>
                      </div>
                    )}
                    {progress && progress.done > 0 && (
                      <div className="retranslate">
                        <div className="row">
                          <span className="muted">{t("translate.retranslate")}:</span>
                          <button className="ghost danger" disabled={progress.running} onClick={() => reTranslate(null)}>{t("translate.retranslateAll")}</button>
                          <button className="ghost danger" disabled={progress.running} onClick={() => reTranslate(reFrom)}>{t("translate.retranslateFrom")}</button>
                          <input type="number" min={1} max={progress.total} value={reFrom}
                            onChange={(e) => setReFrom(Math.max(1, Number(e.target.value) || 1))} style={{ width: 80 }} />
                        </div>
                        <div className="muted resume-hint">{t("translate.retranslateHint")}</div>
                      </div>
                    )}
                  </Panel>
                </>
              )}

              {view === "glossary" && (
                <Panel id="glossary" title={t("glossary.title", { n: glossary.length })} extra={
                  <>
                    <button className="primary" disabled={!pendingCount || !(progress && progress.done > 0)}
                      title={t("glossary.updateTranslationTip")} onClick={updateTranslation}>
                      {t("glossary.updateTranslation")}{pendingCount ? ` (${pendingCount})` : ""}
                    </button>
                    <input placeholder={t("glossary.filter")} value={glossaryQuery} onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 140 }} />
                    <button onClick={refreshGlossary}>↻</button>
                  </>
                }>
                  <div className="table-wrap">
                    <table>
                      <thead><tr><th>{t("glossary.colSource")}</th><th>{t("glossary.colTranslation")}</th><th>{t("glossary.colKind")}</th><th>{t("glossary.colCount")}</th><th>{t("glossary.colActions")}</th></tr></thead>
                      <tbody>
                        <tr className="add-row">
                          <td><input placeholder={t("glossary.addSource")} value={newTerm.source} onChange={(e) => setNewTerm({ ...newTerm, source: e.target.value })} /></td>
                          <td><input placeholder={t("glossary.addTranslation")} value={newTerm.target} onChange={(e) => setNewTerm({ ...newTerm, target: e.target.value })}
                            onKeyDown={(e) => e.key === "Enter" && addTerm()} /></td>
                          <td>
                            <select value={newTerm.kind} onChange={(e) => setNewTerm({ ...newTerm, kind: e.target.value })}>
                              {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
                            </select>
                          </td>
                          <td colSpan={2}><button onClick={addTerm} disabled={!newTerm.source.trim() || !newTerm.target.trim()}>{t("glossary.add")}</button></td>
                        </tr>
                        {filteredGlossary.map((term) => {
                          const dirty = !!pending[term.source];
                          return (
                          <tr key={term.source} className={dirty ? "dirty" : ""}>
                            <td><input defaultValue={term.source} onBlur={(ev) => { const v = ev.target.value.trim(); if (v && v !== term.source) renameTerm(term, v); }} /></td>
                            <td><input key={term.target} defaultValue={term.target} onBlur={(ev) => editTarget(term, ev.target.value)} /></td>
                            <td>
                              <select value={term.kind} onChange={(ev) => editKind(term, ev.target.value)}>
                                {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
                              </select>
                            </td>
                            <td>{term.frequency}{term.pinned ? " 📌" : ""}</td>
                            <td><button className="ghost del" title={t("glossary.delete")} onClick={() => deleteTerm(term)}>✕</button></td>
                          </tr>
                        );})}
                        {filteredGlossary.length === 0 && <tr><td colSpan={5} className="empty">{t("glossary.empty")}</td></tr>}
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
                          {c.origin === "reference" ? "◆ " : c.status === "done" ? "✓ " : "· "}{c.title.slice(0, 60)}
                        </option>
                      ))}
                    </select>
                    <button className="ghost" disabled={!chapters.length} onClick={() => {
                      const i = chapters.findIndex((c) => c.idx === chapterIdx); if (i >= 0 && i < chapters.length - 1) setChapterIdx(chapters[i + 1].idx);
                    }}>›</button>
                    <div className="menu-spacer" />
                    <label className="check"><input type="checkbox" checked={hl} onChange={(e) => setHl(e.target.checked)} /> {t("reader.highlight")}</label>
                    <button className={`chip ${panes.orig ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, orig: !p.orig }))}>{t("reader.original")}</button>
                    <button className={`chip ${panes.transl ? "on" : ""}`} onClick={() => setPanes((p) => ({ ...p, transl: !p.transl }))}>{t("reader.translation")}</button>
                  </div>

                  <div className="panes">
                    {panes.orig && (
                      <div className="pane">
                        <div className="pane-head">
                          <span>{t("reader.original")} {chapter?.number != null && `· #${chapter.number}`}</span>
                          <button className="icon" onClick={() => setPanes((p) => ({ ...p, orig: false }))}>×</button>
                        </div>
                        <div className="pane-body">
                          {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : <>
                            <div className="chtitle">{chapter?.source_title}</div>
                            <div className="chtext">{chapter ? (hl ? highlight(chapter.source, sourceTerms) : chapter.source) : ""}</div>
                          </>}
                        </div>
                      </div>
                    )}
                    {panes.transl && (
                      <div className="pane">
                        <div className="pane-head">
                          <span>{t("reader.translation")} {chapter?.status !== "done" && `· ${t("reader.notTranslated")}`}</span>
                          {chapter?.origin === "reference" && <span className="ref-badge" title={t("reader.fromReferenceTip")}>{t("reader.fromReference")}</span>}
                          <div className="menu-spacer" />
                          <button className="icon" onClick={() => setPanes((p) => ({ ...p, transl: false }))}>×</button>
                        </div>
                        <div className="pane-body">
                          {chapterLoading ? <div className="loading"><span className="spinner" /> {t("reader.loading")}</div> : <>
                            <div className="chtitle">{chapter?.translated_title}</div>
                            <div className="chtext">
                              {chapter?.translated ? (hl ? highlight(chapter.translated, targetTerms) : chapter.translated) : <span className="muted">{t("reader.notTranslatedYet")}</span>}
                            </div>
                          </>}
                        </div>
                      </div>
                    )}
                    {!panes.orig && !panes.transl && <div className="muted" style={{ padding: 20 }}>{t("reader.bothClosed")}</div>}
                  </div>
                </div>
              )}
            </>
          )}
          </>)}
        </main>

        {showConsole && (
          <section className="console">
            <div className="console-head">
              <span className="console-title">{t("console.title")}</span>
              <div className="menu-spacer" />
              <button className="icon" title={t("console.clear")} onClick={() => setLog([])}>⌫</button>
              <button className="icon" onClick={() => setShowConsole(false)}>×</button>
            </div>
            <div className="log" ref={logRef}>
              {log.length === 0
                ? <div className="muted">{t("console.empty")}</div>
                : log.map((l, i) => <div key={i}>{l}</div>)}
            </div>
          </section>
        )}
        </div>
      </div>
    </div>
  );
}
