import { useEffect, useRef, useState } from "react";
import { makeCall } from "./api";
import { Console } from "./components/Console";
import { GlossaryView } from "./components/GlossaryView";
import { About } from "./components/About";
import { Legend } from "./components/Legend";
import { LanguageSetup } from "./components/LanguageSetup";
import { Menubar, type MenuId } from "./components/Menubar";
import { Overview } from "./components/Overview";
import { Reader } from "./components/Reader";
import { Settings } from "./components/Settings";
import { SearchPanel } from "./components/SearchPanel";
import { Sidebar } from "./components/Sidebar";
import { Welcome } from "./components/Welcome";
import { ActivityBar } from "./components/shell/ActivityBar";
import { AssistantPanel } from "./components/shell/AssistantPanel";
import { BottomPanel } from "./components/shell/BottomPanel";
import { StatusBar } from "./components/shell/StatusBar";
import { TabBar } from "./components/shell/TabBar";
import { CommandPalette, type Command } from "./components/CommandPalette";
import { ResizeHandle } from "./components/common/ResizeHandle";
import { useAssistant } from "./hooks/useAssistant";
import { useConsoleLog } from "./hooks/useConsoleLog";
import { useFindReplace } from "./hooks/useFindReplace";
import { useHotkeys } from "./hooks/useHotkeys";
import { useTabs } from "./hooks/useTabs";
import type { FindOpts } from "./lib/find";
import { useBookWorkspace } from "./hooks/useBookWorkspace";
import { useGlossary } from "./hooks/useGlossary";
import { useProjectActions } from "./hooks/useProjectActions";
import { useProjectList } from "./hooks/useProjectList";
import { useTranslationJob } from "./hooks/useTranslationJob";
import { LS_LANG, normalizeLang, translate, type Lang } from "./i18n";
import type { RefInfo, ViewId } from "./types";

export default function App() {
  const [view, setView] = useState<ViewId>("overview");
  const [lang, setLang] = useState<Lang>(() => normalizeLang(localStorage.getItem(LS_LANG)));
  const [srcLang, setSrcLang] = useState("Chinese");
  const [tgtLang, setTgtLang] = useState("Russian");
  // Glossary highlighting is a preference, not a per-book state: it lives in the
  // settings DB and is toggled from Settings (or the View menu).
  const [hl, setHl] = useState(true);
  const [showSettings, setShowSettings] = useState(false);
  const t = (key: string, vars?: Record<string, string | number>) => translate(lang, key, vars);

  const [error, setError] = useState<string | null>(null);
  const [busyById, setBusyById] = useState<Record<string, string | null>>({});
  const setBusyFor = (id: string, msg: string | null) =>
    setBusyById((all) => ({ ...all, [id]: msg }));

  const [menu, setMenu] = useState<MenuId | null>(null);
  const [showAbout, setShowAbout] = useState(false);
  const [showLegend, setShowLegend] = useState(false);
  const [limit, setLimit] = useState<number | "">("");
  const [sidebar, setSidebar] = useState(true);
  // Which panel the sidebar shows, VS Code style: the file tree or search.
  const [sidebarView, setSidebarView] = useState<"explorer" | "search">("explorer");
  const [searchFocus, setSearchFocus] = useState(0);
  const [showConsole, setShowConsole] = useState(true);
  const [showAssistant, setShowAssistant] = useState(() => {
    try { return localStorage.getItem("bc.assistant.open") !== "0"; } catch { return true; }
  });
  const [assistantWidth, setAssistantWidth] = useState(() => {
    try {
      const n = Number(localStorage.getItem("bc.assistant.width"));
      return Number.isFinite(n) && n >= 260 && n <= 560 ? n : 320;
    } catch { return 320; }
  });
  const assistantWidthOrigin = useRef(assistantWidth);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (k: string) => setCollapsed((c) => ({ ...c, [k]: !c[k] }));

  const logRef = useRef<HTMLDivElement>(null);

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
    addLog(tRef.current("log.error", { msg: errText(msg) }));
  }
  const call = makeCall((msg, critical) => {
    logError(msg);
    if (critical) setError(errText(msg));
  });

  // The hook order below is the app's data flow, not an accident. The console
  // owns itself and depends on nothing, so everything can write to it. The
  // project list comes next, because every other hook keys off `activeId`. The
  // actions that operate on a project come last, because they drive all the
  // hooks in between.
  const { linesOf, addLogTo, addLogToRef, clearLog, dropLog } = useConsoleLog();
  const list = useProjectList({ call });
  const { projects, active, setActive, activeProject, activeId } = list;
  const addLog = (m: string) => addLogTo(activeId, m);
  const log = linesOf(activeId);

  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  const book = useBookWorkspace({ call, activeId });
  const glossary = useGlossary({ call, activeId, setBusyFor, setError, addLog, logError, t });
  const job = useTranslationJob({
    call, activeId, book: book.book, t, tRef, activeIdRef,
    addLog, addLogToRef, dropLog,
    chapterIdxRef: book.chapterIdxRef,
    setBusyFor, setError, setPending: glossary.setPending,
    refreshGlossary: glossary.refreshGlossary,
    openChapter: book.openChapter,
    loadChapters: book.loadChapters,
    applyChapterEdit: book.applyChapterEdit,
    applyChapterTitle: book.applyChapterTitle,
    // Logs speak in book chapter numbers, never reading-order indices.
    chapterNumberOf: (idx) => book.chapters.find((c) => c.idx === idx)?.number ?? null,
    errText, limit,
  });
  const {
    openBook, removeProject, openReference, saveProject, openProjectArchive,
    generateSummary, exportAs, langSetup, confirmLangSetup, cancelLangSetup,
  } = useProjectActions({
    call, t, errText, addLog, addLogTo, logError,
    setBusyFor, setBusyById, setError, setView, setMenu,
    list, book, glossary, job,
  });

  const readerFlushRef = useRef<null | (() => Promise<void>)>(null);
  const assistant = useAssistant({
    call,
    activeId,
    enabled: !!activeProject,
    onInvalidated: async (areas) => {
      if (areas.includes("glossary")) await glossary.refreshGlossary();
      if (areas.includes("chapters")) await book.loadChapters();
      if (areas.includes("progress")) await job.refreshProgress();
      if (areas.includes("book_details")) await book.refreshDetails();
      if (areas.includes("reference")) {
        const info = await call<RefInfo | null>("get_reference_info", { projectId: activeId });
        if (info) book.setRef(info);
      }
      if (areas.includes("open_chapter") && book.chapterIdx != null) {
        await readerFlushRef.current?.();
        await book.openChapter(book.chapterIdx);
      }
    },
  });

  // Find/replace lives here (not in Reader) so the Edit menu, the command
  // palette and the shortcuts can open it even from another view.
  const find = useFindReplace();

  /** ⌘⇧F: book-wide search in the sidebar, focused even if it is already open. */
  function openBookSearch() {
    if (!activeProject) return;
    setShowSettings(false);
    setSidebar(true);
    setSidebarView("search");
    setSearchFocus((n) => n + 1);
  }

  /** Jump from a search result to the chapter, with the query armed in the bar. */
  function openSearchHit(idx: number, query: string, opts: FindOpts) {
    tabs.openChapter(idx);
    find.setQuery(query);
    find.setMatchCase(opts.matchCase);
    find.setWholeWord(opts.wholeWord);
    find.setRegex(!!opts.regex);
    find.setCurrent(0);
    find.setOpen(true);
  }

  function openFind(mode: "find" | "replace") {
    if (!activeProject) return;
    setView("reader");
    setShowSettings(false);
    find.openBar(mode);
  }

  const tabs = useTabs({
    view, setView,
    chapterIdx: book.chapterIdx, setChapterIdx: book.setChapterIdx,
    activeId,
  });

  function stepChapter(delta: number) {
    const cs = book.chapters;
    const i = cs.findIndex((c) => c.idx === book.chapterIdx);
    const j = i + delta;
    if (i >= 0 && j >= 0 && j < cs.length) tabs.openChapter(cs[j].idx);
  }

  // Jump from a highlighted term in the reader to its glossary entry.
  function openGlossaryTerm(source: string) {
    glossary.setKindFilter("all");
    glossary.setGlossaryQuery(source);
    setView("glossary");
  }

  // Literal find/replace across every stored translation, then reload the reader.
  async function replaceInBook(findStr: string, replaceStr: string, opts: FindOpts): Promise<number> {
    const n = await call<number>("replace_in_book", {
      projectId: activeId, find: findStr, replace: replaceStr,
      matchCase: opts.matchCase, wholeWord: opts.wholeWord, regex: !!opts.regex,
    });
    if (n != null) {
      addLog(t("log.replacedInBook", { n }));
      await book.loadChapters();
      if (book.chapterIdx != null) await book.openChapter(book.chapterIdx);
    }
    return n ?? 0;
  }


  // Prefer the active project's overlay. While a new book is still importing
  // it has a busy id that is not active yet — show that, and nothing else.
  // A leftover "Opening…" from a superseded project must not cover the UI.
  const listedIds = new Set(projects.map((p) => p.id));
  const importBusy = Object.entries(busyById).find(
    ([id, msg]) => !!msg && !listedIds.has(id),
  )?.[1] ?? null;
  const busy = (activeId ? busyById[activeId] : null) ?? importBusy ?? null;
  const { progress, progressById } = job;
  const canExport = !!progress && progress.done > 0;

  const paletteCommands: Command[] = [
    { id: "overview", label: t("palette.goOverview"), run: () => setView("overview") },
    { id: "glossary", label: t("palette.goGlossary"), run: () => setView("glossary") },
    ...(activeProject
      ? [
          { id: "translate", label: t("palette.translateChapter"), hint: "⌘⏎", run: () => { if (book.chapterIdx != null) job.translateChapter(book.chapterIdx); } },
          { id: "translateTitle", label: t("palette.translateTitle"), run: () => { if (book.chapterIdx != null) void job.translateChapterTitle(book.chapterIdx); } },
          { id: "start", label: t("palette.start"), run: () => job.start() },
          { id: "pause", label: t("palette.pause"), run: () => job.pause() },
          { id: "toggleOriginal", label: t("palette.toggleOriginal"), run: () => book.setPanes((p) => ({ ...p, orig: !p.orig })) },
          { id: "find", label: t("palette.find"), hint: "⌘F", run: () => openFind("find") },
          { id: "searchBook", label: t("palette.searchBook"), hint: "⌘⇧F", run: () => openBookSearch() },
          { id: "replace", label: t("palette.replace"), hint: "⌘H", run: () => openFind("replace") },
          { id: "reference", label: t("palette.openReference"), run: () => openReference() },
          { id: "export-fb2", label: t("palette.exportAs", { fmt: "FB2" }), run: () => exportAs("fb2") },
          { id: "export-epub", label: t("palette.exportAs", { fmt: "EPUB" }), run: () => exportAs("epub") },
          { id: "export-pdf", label: t("palette.exportAs", { fmt: "PDF" }), run: () => exportAs("pdf") },
          { id: "export-txt", label: t("palette.exportAs", { fmt: "TXT" }), run: () => exportAs("txt") },
        ]
      : []),
    { id: "legend", label: t("palette.legend"), run: () => setShowLegend(true) },
    { id: "settings", label: t("palette.settings"), hint: "⌘,", run: () => setShowSettings(true) },
    { id: "toggleConsole", label: t("palette.toggleConsole"), hint: "⌘J", run: () => setShowConsole((s) => !s) },
    { id: "toggleAssistant", label: t("palette.toggleAssistant"), hint: "⌘L", run: () => setShowAssistant((s) => !s) },
  ];

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
      const h = await call<string | null>("get_setting", { key: "highlight_terms" });
      if (h) setHl(h !== "false");
      langLoaded.current = true;
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  function changeSourceLang(v: string) { setSrcLang(v); void call("set_setting", { key: "source_lang", value: v }); }
  function changeTargetLang(v: string) { setTgtLang(v); void call("set_setting", { key: "target_lang", value: v }); }
  function changeHighlight(v: boolean) { setHl(v); void call("set_setting", { key: "highlight_terms", value: String(v) }); }
  useEffect(() => {
    localStorage.setItem(LS_LANG, lang);
    document.documentElement.lang = lang;
    if (langLoaded.current) void call("set_setting", { key: "lang", value: lang });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang]);
  useEffect(() => logRef.current?.scrollTo(0, logRef.current.scrollHeight), [log]);
  // Projects the startup reconcile found on disk but not in the stored list.
  useEffect(() => {
    if (list.recovered > 0) addLog(t("log.projectsRecovered", { n: list.recovered }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [list.recovered]);

  // Refresh on entering the reader. The initial load happens during project
  // activation (see useProjects): list_chapters needs the backend session that
  // open_project registers, so it cannot run in parallel with it.
  useEffect(() => {
    if (view === "reader") void book.loadChapters();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view]);

  useHotkeys({
    "mod+p": (e) => { e.preventDefault(); setPaletteOpen((o) => !o); },
    "mod+b": (e) => { e.preventDefault(); setSidebar((s) => !s); },
    "mod+j": (e) => { e.preventDefault(); setShowConsole((s) => !s); },
    "mod+l": (e) => {
      e.preventDefault();
      setShowAssistant((s) => {
        localStorage.setItem("bc.assistant.open", s ? "0" : "1");
        return !s;
      });
    },
    "mod+,": (e) => { e.preventDefault(); setShowSettings((s) => !s); },
    "alt+arrowdown": (e) => { if (view === "reader") { e.preventDefault(); stepChapter(1); } },
    "alt+arrowup": (e) => { if (view === "reader") { e.preventDefault(); stepChapter(-1); } },
    "mod+f": (e) => { if (activeProject) { e.preventDefault(); openFind("find"); } },
    "mod+shift+f": (e) => { if (activeProject) { e.preventDefault(); openBookSearch(); } },
    "mod+h": (e) => { if (activeProject) { e.preventDefault(); openFind("replace"); } },
    "mod+enter": (e) => {
      if (view === "reader" && book.chapterIdx != null && !progress?.running) {
        e.preventDefault();
        job.translateChapter(book.chapterIdx);
      }
    },
  });

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
        onToggleHighlight={() => changeHighlight(!hl)}
        onToggleConsole={() => setShowConsole((s) => !s)}
        onToggleAssistant={() => setShowAssistant((s) => {
          localStorage.setItem("bc.assistant.open", s ? "0" : "1");
          return !s;
        })}
        onOpenCommandPalette={() => setPaletteOpen(true)}
        onFind={() => { openFind("find"); setMenu(null); }}
        onReplace={() => { openFind("replace"); setMenu(null); }}
        onSearchBook={() => { openBookSearch(); setMenu(null); }}
        onOpenSettings={() => { setShowSettings(true); setMenu(null); }}
        onOpenLegend={() => setShowLegend(true)}
        onOpenAbout={() => setShowAbout(true)}
      />

      <div className="body">
        <ActivityBar
          t={t}
          sidebarOpen={sidebar}
          onToggleSidebar={() => {
            if (sidebar && sidebarView === "explorer") setSidebar(false);
            else { setSidebar(true); setSidebarView("explorer"); }
          }}
          sidebarView={sidebarView}
          onShowSearch={openBookSearch}
          settingsOpen={showSettings}
          onToggleSettings={() => setShowSettings((s) => !s)}
          consoleOpen={showConsole}
          onToggleConsole={() => setShowConsole((s) => !s)}
          assistantOpen={showAssistant}
          onToggleAssistant={() => setShowAssistant((s) => {
            localStorage.setItem("bc.assistant.open", s ? "0" : "1");
            return !s;
          })}
        />
        {sidebar && sidebarView === "search" ? (
          <aside className="sidebar">
            <SearchPanel
              t={t} call={call} activeId={activeId}
              focusToken={searchFocus} onOpenHit={openSearchHit}
            />
          </aside>
        ) : (
        <Sidebar
          t={t} sidebar={sidebar}
          projects={projects} active={active} setActive={setActive}
          progressById={progressById} busyById={busyById}
          chapters={book.chapters}
          chaptersLoading={book.chaptersLoading}
          activeChapterIdx={view === "reader" ? book.chapterIdx : null}
          onOpenChapter={tabs.openChapter}
          error={error} setError={setError}
          onRemove={removeProject} onOpenBook={openBook}
        />
        )}

        <div className="rightcol">
          <div className="editor-region">
            {showSettings ? (
              <Settings
                t={t} call={call}
                lang={lang} setLang={setLang}
                srcLang={srcLang} tgtLang={tgtLang}
                onChangeSourceLang={changeSourceLang}
                onChangeTargetLang={changeTargetLang}
                highlight={hl} onChangeHighlight={changeHighlight}
                onClose={() => setShowSettings(false)}
              />
            ) : !activeProject ? (
              <Welcome t={t} onOpenBook={openBook} />
            ) : (
              <>
                <TabBar
                  t={t} view={view}
                  chapters={book.chapters} openChapters={tabs.openChapters}
                  chapterIdx={book.chapterIdx} glossaryCount={glossary.total}
                  onSelectView={setView}
                  onSelectChapter={tabs.openChapter}
                  onCloseChapter={tabs.closeChapter}
                />
                <main className="workarea">
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
                      progress={progress} activeKey={activeId}
                      sample={job.sample} setSample={job.setSample}
                      limit={limit} setLimit={setLimit}
                      reFrom={job.reFrom} setReFrom={job.setReFrom}
                      onTranslateTitle={book.translateTitle}
                      onReplaceCover={book.replaceCover}
                      onGenerateSummary={generateSummary}
                      onSaveSummary={book.saveSummary}
                      onSaveBookPrompt={book.saveBookPrompt}
                      onOpenReference={openReference}
                      onBootstrap={job.bootstrap}
                      onHarvestGlossary={job.harvestGlossary}
                      onStart={job.start} onPause={job.pause}
                      onRefreshProgress={job.refreshProgress}
                      onRetranslate={job.reTranslate}
                    />
                  )}

                  {view === "glossary" && (
                    <GlossaryView
                      t={t} collapsed={collapsed} onToggle={toggle}
                      terms={glossary.terms}
                      total={glossary.total}
                      loading={glossary.loading}
                      glossaryQuery={glossary.glossaryQuery}
                      setGlossaryQuery={glossary.setGlossaryQuery}
                      kindFilter={glossary.kindFilter}
                      setKindFilter={glossary.setKindFilter}
                      onLoadMore={glossary.loadMore}
                      pending={glossary.pending} pendingCount={glossary.pendingCount}
                      progress={progress}
                      onUpdateTranslation={glossary.updateTranslation}
                      onRefreshGlossary={glossary.refreshGlossary}
                      onSaveTerm={glossary.saveTerm}
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
                      hl={hl} find={find}
                      terms={book.chapterTerms}
                      onOpenGlossaryTerm={openGlossaryTerm}
                      onReplaceInBook={replaceInBook}
                      translating={!!progress?.running}
                      onTranslateChapter={job.translateChapter}
                      onTranslateChapterTitle={job.translateChapterTitle}
                      onSaveTranslation={job.saveChapterTranslation}
                      onSaveChapterPrompt={job.saveChapterPrompt}
                      onSaveChapterContext={job.saveChapterContext}
                      onRetranslateWithPrompt={job.retranslateWithPrompt}
                      flushRef={readerFlushRef}
                    />
                  )}
                </main>
              </>
            )}
          </div>

          {showConsole && (
            <BottomPanel>
              <Console
                t={t} log={log} logRef={logRef}
                onClear={() => clearLog(activeId)}
                onClose={() => setShowConsole(false)}
              />
            </BottomPanel>
          )}
        </div>

        {showAssistant && (
          <ResizeHandle
            axis="x"
            onStart={() => { assistantWidthOrigin.current = assistantWidth; }}
            onDrag={(delta) => {
              // Dragging the left edge of the dock: positive delta shrinks it.
              const next = Math.min(560, Math.max(260, assistantWidthOrigin.current - delta));
              setAssistantWidth(next);
              localStorage.setItem("bc.assistant.width", String(next));
            }}
          />
        )}
        <AssistantPanel
          t={t}
          open={showAssistant}
          width={assistantWidth}
          enabled={!!activeProject}
          messages={assistant.messages}
          status={assistant.status}
          pendingConfirm={assistant.pendingConfirm}
          confirmExpired={assistant.confirmExpired}
          onClose={() => {
            setShowAssistant(false);
            localStorage.setItem("bc.assistant.open", "0");
          }}
          onClear={() => void assistant.clear()}
          onSend={(text) => void assistant.sendWithChapter(text, book.chapterIdx)}
          onApprove={(id) => void assistant.approve(id)}
          onDeny={(id) => void assistant.deny(id)}
          onCancel={() => void assistant.cancel()}
        />
      </div>
      <StatusBar
        t={t} progress={progress} book={book.book}
        glossaryCount={glossary.total}
        srcLang={srcLang} tgtLang={tgtLang} busy={busy}
        onToggleConsole={() => setShowConsole((s) => !s)}
      />

      <CommandPalette
        t={t} open={paletteOpen} onClose={() => setPaletteOpen(false)}
        commands={paletteCommands} chapters={book.chapters}
        onOpenChapter={tabs.openChapter}
      />

      {showLegend && <Legend t={t} onClose={() => setShowLegend(false)} />}
      {showAbout && <About t={t} call={call} onClose={() => setShowAbout(false)} />}
      {langSetup && (
        <LanguageSetup
          t={t}
          name={langSetup.name}
          source={langSetup.info.source_lang}
          target={langSetup.info.target_lang}
          detected={langSetup.info.source_detected}
          onConfirm={(source, target) => void confirmLangSetup(source, target)}
          onCancel={() => void cancelLangSetup()}
        />
      )}
    </div>
  );
}
