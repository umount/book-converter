import { useEffect, useRef, useState } from "react";
import { makeCall } from "./api";
import { Console } from "./components/Console";
import { GlossaryView } from "./components/GlossaryView";
import { Menubar } from "./components/Menubar";
import { Overview } from "./components/Overview";
import { Reader } from "./components/Reader";
import { Settings } from "./components/Settings";
import { Sidebar } from "./components/Sidebar";
import { Welcome } from "./components/Welcome";
import { useBookWorkspace } from "./hooks/useBookWorkspace";
import { useGlossary } from "./hooks/useGlossary";
import { useProjects, type ProjectHelpers } from "./hooks/useProjects";
import { useTranslationJob } from "./hooks/useTranslationJob";
import { LS_LANG, normalizeLang, translate, type Lang } from "./i18n";
import type { ViewId } from "./types";

export default function App() {
  const [view, setView] = useState<ViewId>("overview");
  const [lang, setLang] = useState<Lang>(() => normalizeLang(localStorage.getItem(LS_LANG)));
  const [srcLang, setSrcLang] = useState("Chinese");
  const [tgtLang, setTgtLang] = useState("Russian");
  const [showSettings, setShowSettings] = useState(false);
  const t = (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars);

  const [error, setError] = useState<string | null>(null);
  const [busyById, setBusyById] = useState<Record<string, string | null>>({});
  const setBusyFor = (id: string, msg: string | null) =>
    setBusyById((all) => ({ ...all, [id]: msg }));

  const [menu, setMenu] = useState<"file" | "view" | null>(null);
  const [limit, setLimit] = useState<number | "">("");
  const [sidebar, setSidebar] = useState(true);
  const [showConsole, setShowConsole] = useState(true);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (k: string) => setCollapsed((c) => ({ ...c, [k]: !c[k] }));

  const logRef = useRef<HTMLDivElement>(null);
  const helpersRef = useRef<ProjectHelpers>(null!);

  // Log/error bridge filled after useTranslationJob mounts (call/glossary need it earlier).
  const logApiRef = useRef<{
    addLog: (m: string) => void;
    addLogTo: (id: string, m: string) => void;
  }>({ addLog: () => {}, addLogTo: () => {} });

  // Keep live refs so once-registered event listeners / IPC see current values.
  const tRef = useRef(t);
  tRef.current = t;

  // Map a stable backend error code (e.g. "no_source") to a localized message;
  // pass anything else through unchanged.
  function errText(raw: string): string {
    const key = `err.${raw.trim()}`;
    const s = tRef.current(key);
    return s === key ? raw : s;
  }
  function logError(msg: string) {
    logApiRef.current.addLog(tRef.current("log.error", { msg: errText(msg) }));
  }

  const call = makeCall((msg, critical) => {
    logError(msg);
    if (critical) setError(errText(msg));
  });

  const projectsApi = useProjects(helpersRef);
  const { projects, active, setActive, activeProject, activeId,
    openBook, removeProject, openReference, saveProject, openProjectArchive,
    generateSummary, exportAs } = projectsApi;

  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  const book = useBookWorkspace({ call, activeId });
  const glossary = useGlossary({
    call, activeId, setBusyFor, setError,
    addLog: (m) => logApiRef.current.addLog(m),
    logError, t,
  });
  const job = useTranslationJob({
    call, activeId, book: book.book, t, tRef, activeIdRef,
    chapterIdxRef: book.chapterIdxRef,
    setBusyFor, setError, setPending: glossary.setPending,
    refreshGlossary: glossary.refreshGlossary,
    openChapter: book.openChapter,
    loadChapters: book.loadChapters,
    errText, limit,
  });

  logApiRef.current = { addLog: job.addLog, addLogTo: job.addLogTo };

  helpersRef.current = {
    call, t, setBusyFor, setBusyById, setError, logError,
    addLog: job.addLog, addLogTo: job.addLogTo,
    clearWorkspace: book.clearWorkspace,
    setBook: book.setBook, setRef: book.setRef,
    setGlossary: glossary.setGlossary, setPending: glossary.setPending,
    refreshDetails: book.refreshDetails,
    refreshProgressFor: job.refreshProgressFor,
    refreshProgress: job.refreshProgress,
    refreshGlossary: glossary.refreshGlossary,
    translateTitle: book.translateTitle,
    loadChapters: book.loadChapters,
    openChapter: book.openChapter,
    chapterIdxRef: book.chapterIdxRef,
    clearProjectJobState: job.clearProjectJobState,
    setView, setMenu, errText,
  };

  // Prefer the active project's busy message; fall back to any in-flight busy
  // (e.g. openBook sets busy for a new id before setActive makes it active).
  const busy =
    (activeId ? busyById[activeId] : null) ??
    Object.values(busyById).find((m): m is string => !!m) ??
    null;
  const { progress, log, progressById } = job;
  const canExport = !!progress && progress.done > 0;

  // Language: localStorage is an instant cache to avoid a flash on load; the DB
  // is the durable source of truth (survives restarts). Load DB once on mount,
  // then persist every change to both.
  const langLoaded = useRef(false);
  useEffect(() => {
    (async () => {
      const v = await call<string | null>("get_setting", { key: "lang" });
      if (v) setLang(normalizeLang(v));
      const s = await call<string | null>("get_setting", { key: "source_lang" });
      if (s) setSrcLang(s);
      const g = await call<string | null>("get_setting", { key: "target_lang" });
      if (g) setTgtLang(g);
      langLoaded.current = true;
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  function changeSourceLang(v: string) { setSrcLang(v); void call("set_setting", { key: "source_lang", value: v }); }
  function changeTargetLang(v: string) { setTgtLang(v); void call("set_setting", { key: "target_lang", value: v }); }
  useEffect(() => {
    localStorage.setItem(LS_LANG, lang);
    document.documentElement.lang = lang;
    if (langLoaded.current) void call("set_setting", { key: "lang", value: lang });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang]);
  useEffect(() => logRef.current?.scrollTo(0, logRef.current.scrollHeight), [log]);

  useEffect(() => {
    if (view === "reader") void book.loadChapters();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view]);

  return (
    <div className="ide">
      <Menubar
        t={t} menu={menu} setMenu={setMenu} busy={busy} canExport={canExport}
        hasActive={!!activeProject}
        onOpenBook={openBook} onOpenReference={openReference}
        onOpenProject={openProjectArchive} onSaveProject={saveProject}
        onExport={exportAs}
        onToggleSidebar={() => setSidebar((s) => !s)}
        onShowBothPanes={() => book.setPanes({ orig: true, transl: true })}
        onToggleHighlight={() => book.setHl((h) => !h)}
        onToggleConsole={() => setShowConsole((s) => !s)}
        onOpenSettings={() => { setShowSettings(true); setMenu(null); }}
      />

      <div className="body">
        <Sidebar
          t={t} sidebar={sidebar} setSidebar={setSidebar}
          projects={projects} active={active} setActive={setActive}
          view={view} setView={setView}
          progressById={progressById} busyById={busyById}
          glossaryCount={glossary.glossary.length}
          error={error} setError={setError}
          onRemove={removeProject} onOpenBook={openBook}
        />

        <div className="rightcol">
          <main className="workarea">
            {showSettings ? (
              <Settings
                t={t} collapsed={collapsed} onToggle={toggle}
                lang={lang} setLang={setLang}
                srcLang={srcLang} tgtLang={tgtLang}
                onChangeSourceLang={changeSourceLang}
                onChangeTargetLang={changeTargetLang}
                onClose={() => setShowSettings(false)}
              />
            ) : (<>
              {!activeProject ? (
                <Welcome t={t} onOpenBook={openBook} />
              ) : (
                <>
                  <div className="workhead">
                    <div className="worktitle">{book.details?.title_translated || book.details?.title || activeProject.name}</div>
                    <div className="worksub">
                      {book.book && `${t("overview.chapters", { n: book.book.total_chapters })} · ${book.book.format} · ${book.book.encoding}`}
                      {book.ref && ` · ${t("overview.ref", { n: book.ref.max_covered ?? "?" })}`}
                      {progress && ` · ${t("overview.done", { done: progress.done, total: progress.total })}`}
                    </div>
                  </div>

                  {view === "overview" && (
                    <Overview
                      t={t} collapsed={collapsed} onToggle={toggle}
                      refInfo={book.ref} details={book.details}
                      progress={progress} activeKey={active}
                      sample={job.sample} setSample={job.setSample}
                      limit={limit} setLimit={setLimit}
                      reFrom={job.reFrom} setReFrom={job.setReFrom}
                      onTranslateTitle={book.translateTitle}
                      onReplaceCover={book.replaceCover}
                      onGenerateSummary={generateSummary}
                      onSaveSummary={book.saveSummary}
                      onOpenReference={openReference}
                      onBootstrap={job.bootstrap}
                      onStart={job.start} onPause={job.pause}
                      onRefreshProgress={job.refreshProgress}
                      onRetranslate={job.reTranslate}
                    />
                  )}

                  {view === "glossary" && (
                    <GlossaryView
                      t={t} collapsed={collapsed} onToggle={toggle}
                      glossary={glossary.glossary}
                      filteredGlossary={glossary.filteredGlossary}
                      glossaryQuery={glossary.glossaryQuery}
                      setGlossaryQuery={glossary.setGlossaryQuery}
                      newTerm={glossary.newTerm} setNewTerm={glossary.setNewTerm}
                      pending={glossary.pending} pendingCount={glossary.pendingCount}
                      progress={progress}
                      onUpdateTranslation={glossary.updateTranslation}
                      onRefreshGlossary={glossary.refreshGlossary}
                      onAddTerm={glossary.addTerm}
                      onRenameTerm={glossary.renameTerm}
                      onEditTarget={glossary.editTarget}
                      onEditKind={glossary.editKind}
                      onDeleteTerm={glossary.deleteTerm}
                    />
                  )}

                  {view === "reader" && (
                    <Reader
                      t={t}
                      chapters={book.chapters}
                      chapterIdx={book.chapterIdx}
                      setChapterIdx={book.setChapterIdx}
                      chapter={book.chapter}
                      chapterLoading={book.chapterLoading}
                      panes={book.panes} setPanes={book.setPanes}
                      hl={book.hl} setHl={book.setHl}
                      sourceTerms={glossary.sourceTerms}
                      targetTerms={glossary.targetTerms}
                      translating={!!progress?.running}
                      onTranslateChapter={job.translateChapter}
                      onSaveTranslation={job.saveChapterTranslation}
                    />
                  )}
                </>
              )}
            </>)}
          </main>

          {showConsole && (
            <Console
              t={t} log={log} logRef={logRef}
              onClear={() => activeId && job.setLogsById((all) => ({ ...all, [activeId]: [] }))}
              onClose={() => setShowConsole(false)}
            />
          )}
        </div>
      </div>
    </div>
  );
}
