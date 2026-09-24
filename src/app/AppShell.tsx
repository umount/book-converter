import { ToolbarIcon } from "../shared/ui/ToolbarIcon";
import { BookSearch } from "../features/book/BookSearch";
import { Assistant } from "../features/assistant/Assistant";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { projectApi as api } from "../shared/api/projects";
import { WorkspaceStore } from "../shared/state/workspace";
import { JobStore } from "../shared/state/jobs";
import { BookEditorSession } from "../shared/state/editor";
import type { ProjectSummary } from "../shared/contracts/generated";
import { VirtualList } from "../shared/ui/VirtualList";
import { CreateProject } from "../features/projects/CreateProject";
import { BookReader } from "../features/book/BookReader";
import { BookTools, type BookTool } from "../features/book/BookTools";
import { Glossary } from "../features/glossary/Glossary";
import { MangaWorkspace } from "../features/manga/MangaWorkspace";
import { ProjectLibrary } from "../features/projects/ProjectLibrary";
import { ArchiveExport } from "../features/projects/ArchiveExport";
import { JobPanel } from "./JobPanel";
import { CommandPalette, type Command } from "./CommandPalette";
import { Settings } from "./Settings";
import { previewMode } from "../shared/api/desktop";
import { translator, errorText, languageName } from "./strings";
import { LS_LANG, normalizeLang } from "../i18n";
type Runtime = {
  workspace: WorkspaceStore;
  jobs: JobStore;
};
export default function AppShell() {
  const [runtime, setRuntime] = useState<Runtime | null>(null),
    [error, setError] = useState<unknown>(null);
  useEffect(() => {
    const value = {
      workspace: new WorkspaceStore(api),
      jobs: new JobStore(api, setError),
    };
    setRuntime(value);
    return () => {
      value.workspace.dispose();
      value.jobs.dispose();
    };
  }, []);
  return runtime ? (
    <Shell
      runtime={runtime}
      initialError={error}
      dismissInitialError={() => setError(null)}
    />
  ) : null;
}
function Shell({
  runtime: { workspace, jobs },
  initialError,
  dismissInitialError,
}: {
  runtime: Runtime;
  initialError: unknown;
  dismissInitialError: () => void;
}) {
  const state = useSyncExternalStore(workspace.subscribe, workspace.snapshot);
  const [lang, setLang] = useState(() =>
      normalizeLang(previewMode ? "ru" : localStorage.getItem(LS_LANG)),
    ),
    t = translator(lang);
  const [catalog, setCatalog] = useState<ProjectSummary[]>([]),
    [library, setLibrary] = useState(true),
    [create, setCreate] = useState(false),
    [settings, setSettings] = useState(false);
  const [panel, setPanel] = useState<"reader" | "glossary" | BookTool>(
      "reader",
    ),
    [filter, setFilter] = useState(""),
    [showJobs, setShowJobs] = useState(false),
    [force, setForce] = useState(false),
    [extractGlossary, setExtractGlossary] = useState(true),
    [batchSizes, setBatchSizes] = useState<Record<string, string>>(() => {
      try {
        return JSON.parse(localStorage.getItem("bc.batchSizes") || "{}") || {};
      } catch {
        return {};
      }
    });
  const batchSize = state.project
    ? (batchSizes[state.project.id] ?? "10")
    : "10";
  const validBatchSize =
    /^\d+$/.test(batchSize) &&
    Number(batchSize) > 0 &&
    Number(batchSize) <= 4294967295;
  const [chapterState, setChapterState] = useState("all");
  const [focusBlock, setFocusBlock] = useState<string | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(initialError),
    [, redraw] = useState(0);
  const editor = useMemo(
    () =>
      state.project && state.chapter
        ? new BookEditorSession(api, state.project.id, state.chapter)
        : null,
    [state.project, state.chapter],
  );
  const editorRef = useRef<BookEditorSession | null>(null);
  const [palette, setPalette] = useState(false);
  const lock = useRef(false);
  const toolFlush = useRef<(() => Promise<void>) | null>(null);
  const [showSearch, setShowSearch] = useState(false);
  const [toolsVersion, setToolsVersion] = useState(0);
  const [showAssistant, setShowAssistant] = useState(
    () => window.innerWidth > 1000,
  );
  const assistantFlush = useRef<(() => Promise<void>) | null>(null);
  const registerAssistantFlush = useCallback(
    (flush: (() => Promise<void>) | null) => {
      assistantFlush.current = flush;
    },
    [],
  );
  const registerFlush = useCallback((flush: (() => Promise<void>) | null) => {
    toolFlush.current = flush;
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false,
      stop: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => {
        event.preventDefault();
        void (async () => {
          await editorRef.current?.flush();
          await toolFlush.current?.();
          await assistantFlush.current?.();
          await getCurrentWindow().destroy();
        })().catch(setError);
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(setError);
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  async function reload() {
    const next = await api.list();
    setCatalog(next);
    await Promise.all(next.map((p) => jobs.watch(p.descriptor.id)));
  }
  useEffect(() => {
    let alive = true;
    void api
      .list()
      .then((next) => {
        if (alive) {
          setCatalog(next);
          for (const p of next) void jobs.watch(p.descriptor.id);
        }
      })
      .catch(setError);
    return () => {
      alive = false;
    };
  }, [jobs]);
  useEffect(() => {
    editorRef.current = editor;
    return () => {
      editor?.dispose();
    };
  }, [editor]);
  useEffect(() => {
    if (!editor) return;
    return editor.subscribe(() =>
      workspace.updateChapter(editor.snapshot().view.chapter),
    );
  }, [editor, workspace]);
  useEffect(() => {
    const states = new Map<string, string>();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stop = jobs.subscribe(() => {
      redraw((v) => v + 1);
      void editorRef.current?.refresh().catch(setError);
      const id = workspace.snapshot().project?.id;
      let changed = false;
      for (const job of jobs.list(id ?? "")) {
        const key = `${job.job.projectId}/${job.job.jobId}`;
        if (
          states.get(key) !== `${job.state}/${job.revision}` &&
          job.job.projectId === id
        )
          changed = true;
        states.set(key, `${job.state}/${job.revision}`);
      }
      if (changed) {
        clearTimeout(timer);
        timer = setTimeout(
          () => void workspace.refreshChapters().catch(setError),
          200,
        );
      }
    });
    return () => {
      stop();
      clearTimeout(timer);
    };
  }, [jobs, workspace]);
  useEffect(() => {
    const handler = (e: BeforeUnloadEvent) => {
      if (editorRef.current?.snapshot().drafts.size) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (document.querySelector("dialog[open]")) {
        if (["s", "k"].includes(e.key.toLowerCase())) e.preventDefault();
        return;
      }
      if (
        e.key.toLowerCase() === "f" &&
        !library &&
        state.project?.kind === "book"
      ) {
        e.preventDefault();
        setShowSearch(true);
        requestAnimationFrame(() =>
          document
            .querySelector<HTMLInputElement>(".bc-book-search input")
            ?.focus(),
        );
      }
      if (e.key.toLowerCase() === "s") {
        e.preventDefault();
        void act(async () => {}).catch(setError);
      }
      if (e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette(true);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });
  async function act(work: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError(null);
    try {
      await editorRef.current?.flush();
      await toolFlush.current?.();
      await assistantFlush.current?.();
      await work();
    } catch (e) {
      setError(e);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  async function activate(id: string) {
    await workspace.open(id);
    setLibrary(false);
    setPanel("reader");
    setFilter("");
    setChapterState("all");
    await jobs.watch(id);
  }
  async function run(
    kind: "metadata" | "glossary",
    batch?: { maxChapters: number; force: boolean },
  ) {
    if (!state.project) return;
    await editorRef.current?.flush();
    const projectId = state.project.id;
    if (kind === "glossary" && !batch) throw new Error(t("batchCount"));
    const job = await (kind === "metadata"
      ? api.startMetadata({ projectId })
      : api.extractGlossary({
          projectId,
          selection: { kind: "all" },
          maxChapters: batch!.maxChapters,
          force: batch!.force,
        }));
    await jobs.refresh(job);
    setShowJobs(true);
  }
  async function translate(all: boolean) {
    if (!state.project) return;
    if (all && !validBatchSize) return;
    const job = await api.translate({
      projectId: state.project.id,
      selection: all
        ? { kind: "all" }
        : {
            kind: "explicit_ids",
            ids: editor ? [editor.snapshot().view.chapter.id] : [],
          },
      options: {
        force,
        instructions: null,
        maxChapters: all ? Number(batchSize) : 1,
        extractGlossary,
      },
    });
    await jobs.refresh(job);
    setShowJobs(true);
  }
  const project = state.project,
    jobList = catalog.flatMap((p) => jobs.list(p.descriptor.id));
  const translationRunning = jobList.some(
    (job) =>
      job.job.projectId === project?.id &&
      job.kind === "book_translation" &&
      ["queued", "running", "cancelling"].includes(job.state),
  );
  const translatedCount = state.chapters.filter(
    (c) => c.origin !== null,
  ).length;
  const reviewedCount = state.chapters.filter((c) => c.needsReview).length;
  const failedCount = state.chapters.filter(
    (c) => c.status === "failed",
  ).length;
  const visibleChapters = state.chapters.filter(
    (c) =>
      c.title.toLocaleLowerCase().includes(filter.toLocaleLowerCase()) &&
      (chapterState === "all" ||
        (chapterState === "review"
          ? c.needsReview
          : chapterState === "reference"
            ? c.origin === "reference"
            : c.status === chapterState)),
  );
  const tabs =
    project?.kind === "book"
      ? ([
          "reader",
          "overview",
          "glossary",
          "reference",
          "replace",
          "instructions",
          "export",
        ] as const)
      : (["reader", "glossary", "export"] as const);
  const commands: Command[] = [
    {
      id: "library",
      label: t("library"),
      run: () =>
        void act(async () => {
          setLibrary(true);
          await reload();
        }),
    },
    { id: "create", label: t("newProject"), run: () => setCreate(true) },
    { id: "settings", label: t("settings"), run: () => setSettings(true) },
    { id: "jobs", label: t("jobs"), run: () => setShowJobs(true) },
    ...catalog.map((p) => ({
      id: p.descriptor.id,
      label: `${t("open")}: ${p.descriptor.name}`,
      run: () => void act(() => activate(p.descriptor.id)),
    })),
    ...(project
      ? tabs.map((tab) => ({
          id: tab,
          label: t(
            tab === "reader" && project.kind === "manga" ? "pages" : tab,
          ),
          run: () =>
            void act(async () => {
              setLibrary(false);
              setPanel(tab);
            }),
        }))
      : []),
  ];
  return (
    <div className="bc-app">
      {previewMode && (
        <div className="bc-preview-banner">{t("previewMode")}</div>
      )}
      <header className="bc-topbar">
        <span className="bc-brand">
          <img src="/logo.svg" alt="" width="22" height="22" />
          <strong>Book Converter</strong>
        </span>
        <button
          aria-pressed={library}
          onClick={() =>
            void act(async () => {
              setLibrary(true);
              await reload();
            })
          }
        >
          {t("library")}
        </button>
        <button
          className="bc-icon-button"
          aria-label={t("newProject")}
          title={t("newProject")}
          onClick={() => setCreate(true)}
        >
          <ToolbarIcon name="add" />
        </button>
        <span className="bc-spacer" />
        {!library && project?.kind === "book" && (
          <button
            className="bc-icon-button"
            aria-label={t("assistant")}
            title={t("assistant")}
            aria-pressed={showAssistant}
            onClick={() => setShowAssistant((v) => !v)}
          >
            <ToolbarIcon name="assistant" />
          </button>
        )}
        <button
          className="bc-icon-button bc-jobs-toggle"
          aria-label={t("jobs")}
          title={t("jobs")}
          onClick={() => setShowJobs(!showJobs)}
          aria-pressed={showJobs}
        >
          <ToolbarIcon name="jobs" />
          {jobList.some((j) => j.state === "running") && (
            <span className="bc-activity-dot" />
          )}
        </button>
        <button
          className="bc-icon-button"
          aria-label={t("settings")}
          title={t("settings")}
          onClick={() => setSettings(true)}
        >
          <ToolbarIcon name="settings" />
        </button>
      </header>
      {(error ?? state.error ?? initialError) != null && (
        <div className="bc-error" role="alert">
          {errorText(error ?? state.error ?? initialError, t)}
          <button
            aria-label={t("close")}
            onClick={() => {
              setError(null);
              dismissInitialError();
              workspace.clearError();
            }}
          >
            ×
          </button>
        </div>
      )}
      {library || (!project && !state.loading) ? (
        <ProjectLibrary
          catalog={catalog}
          busy={busy}
          t={t}
          create={() => setCreate(true)}
          open={(id) => void act(() => activate(id))}
          importArchive={() =>
            void act(async () => {
              const path = await open({
                multiple: false,
                filters: [{ name: t("importArchive"), extensions: ["bcproj"] }],
              });
              if (typeof path === "string") {
                const p = await api.importArchive({ path });
                await reload();
                await activate(p.id);
              }
            })
          }
          remove={(id) =>
            void act(async () => {
              const p = catalog.find((p) => p.descriptor.id === id)?.descriptor;
              if (
                !p ||
                !(await confirm(`${p.name}\n\n${t("deleteConfirm")}`, {
                  title: t("delete"),
                  kind: "warning",
                }))
              )
                return;
              await api.delete({ projectId: id });
              if (project?.id === id) workspace.close();
              await reload();
            })
          }
        />
      ) : (
        <div className="bc-workspace">
          {project?.kind === "book" && (
            <aside className="bc-sidebar">
              <div className="bc-sidebar-heading">
                <h2>{project.name}</h2>
                <button
                  className="bc-icon-button"
                  aria-label={t("bookSearch")}
                  title={`${t("bookSearch")} (Ctrl+F)`}
                  aria-pressed={showSearch}
                  onClick={() => setShowSearch((v) => !v)}
                >
                  <ToolbarIcon name="search" />
                </button>
              </div>
              <p className="bc-hint">
                {state.settings &&
                  `${languageName(state.settings.languages.source ?? "und", lang)} → ${languageName(state.settings.languages.target, lang)}`}
              </p>
              <BookSearch
                key={project.id}
                projectId={project.id}
                t={t}
                active={showSearch}
                close={() => setShowSearch(false)}
                open={(chapter, block) =>
                  void act(async () => {
                    setFocusBlock(block);
                    await workspace.selectChapter(chapter);
                    setPanel("reader");
                  })
                }
              />
              <div className="bc-chapter-navigation" hidden={showSearch}>
                <details className="bc-chapter-filters" key={project.id}>
                  <summary title={t("chapterFilter")}>
                    <span>
                      {t("chapters")} ·{" "}
                      {filter || chapterState !== "all"
                        ? `${visibleChapters.length} / ${state.chapters.length} ●`
                        : state.chapters.length}
                    </span>
                    <svg
                      aria-hidden="true"
                      width="16"
                      height="16"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.6"
                    >
                      <path d="M4 5h16M7 12h10M10 19h4" />
                    </svg>
                  </summary>
                  <label className="bc-chapter-search">
                    <span className="bc-sr-only">{t("chapterFilter")}</span>
                    <input
                      aria-label={t("chapterFilter")}
                      placeholder={t("chapterFilter")}
                      value={filter}
                      onChange={(e) => setFilter(e.target.value)}
                    />
                  </label>
                  <label className="bc-chapter-search">
                    {t("chapterStatusFilter")}
                    <select
                      value={chapterState}
                      onChange={(e) => setChapterState(e.target.value)}
                    >
                      <option value="all">{t("allChapters")}</option>
                      <option value="pending">{t("chapterPending")}</option>
                      <option value="failed">{t("chapterFailed")}</option>
                      <option value="review">{t("review")}</option>
                      <option value="done">{t("chapterDone")}</option>
                      <option value="reference">{t("originReference")}</option>
                    </select>
                  </label>
                </details>
                <VirtualList
                  key={`${project.id}/${filter}/${chapterState}`}
                  items={visibleChapters}
                  rowHeight={62}
                  className="bc-chapters"
                  renderRow={(c) => (
                    <button
                      key={c.id}
                      disabled={busy}
                      aria-current={
                        editor?.snapshot().view.chapter.id === c.id
                          ? "page"
                          : undefined
                      }
                      onClick={() =>
                        void act(() => workspace.selectChapter(c.id))
                      }
                    >
                      <span>{c.position + 1}</span>
                      <strong>{c.title}</strong>
                      <small>
                        {t(
                          c.status === "failed"
                            ? "chapterFailed"
                            : c.status === "in_progress"
                              ? "chapterInProgress"
                              : c.status === "done"
                                ? "chapterDone"
                                : c.status === "skipped"
                                  ? "chapterSkipped"
                                  : "chapterPending",
                        )}
                        {c.origin
                          ? ` · ${t(c.origin === "reference" ? "originReference" : c.origin === "manual" ? "originManual" : "originModel")}`
                          : ""}
                        {c.needsReview ? ` · ${t("review")}` : ""}
                      </small>
                    </button>
                  )}
                />
              </div>
            </aside>
          )}
          <main className="bc-main">
            <nav className="bc-tabs">
              {tabs.map((tab) => (
                <button
                  key={tab}
                  aria-pressed={panel === tab}
                  disabled={busy}
                  onClick={() => void act(async () => setPanel(tab))}
                >
                  {t(
                    tab === "reader" && project?.kind === "manga"
                      ? "pages"
                      : tab,
                  )}
                </button>
              ))}
            </nav>
            {project?.kind === "book" && panel === "reader" && (
              <div className="bc-toolbar">
                <button
                  className="primary"
                  disabled={busy || translationRunning || !editor}
                  onClick={() => void act(() => translate(false))}
                >
                  {t("translateChapter")}
                </button>
                <details className="bc-translation-options">
                  <summary>{t("translationOptions")}</summary>
                  <label className="bc-check">
                    <input
                      type="checkbox"
                      checked={force}
                      onChange={(e) => setForce(e.target.checked)}
                    />
                    {t("force")}
                  </label>
                  <label className="bc-check">
                    <input
                      type="checkbox"
                      checked={extractGlossary}
                      onChange={(e) => setExtractGlossary(e.target.checked)}
                    />
                    {t("batchGlossary")}
                  </label>{" "}
                </details>
              </div>
            )}
            <div className="bc-content">
              {state.loading ? (
                <p className="bc-empty">{t("loading")}</p>
              ) : (
                project &&
                (panel === "glossary" ? (
                  <Glossary
                    key={project.id}
                    projectId={project.id}
                    t={t}
                    canExtract={project.kind === "book"}
                    onJob={async (job) => {
                      await jobs.refresh(job);
                      setShowJobs(true);
                    }}
                    extract={(maxChapters, force) =>
                      run("glossary", { maxChapters, force })
                    }
                  />
                ) : project.kind === "manga" ? (
                  panel === "export" ? (
                    <ArchiveExport project={project} t={t} />
                  ) : (
                    <MangaWorkspace
                      key={project.id}
                      projectId={project.id}
                      t={t}
                    />
                  )
                ) : panel === "reader" ? (
                  editor ? (
                    <BookReader
                      session={editor}
                      t={t}
                      focusBlock={focusBlock}
                      busy={
                        busy ||
                        jobList.some(
                          (j) =>
                            j.job.projectId === project.id &&
                            ["queued", "running", "cancelling"].includes(
                              j.state,
                            ),
                        )
                      }
                      onTranslateTitle={() =>
                        void act(async () => {
                          const view = editor.snapshot().view;
                          if (!view.translation) return;
                          const job = await api.translateTitle({
                            projectId: project.id,
                            chapterId: view.chapter.id,
                            expectedRevision: view.translation.revision,
                          });
                          await jobs.refresh(job);
                          setShowJobs(true);
                        })
                      }
                    />
                  ) : (
                    <p className="bc-empty">{t("noChapter")}</p>
                  )
                ) : (
                  <BookTools
                    key={`${project.id}/${state.chapter?.chapter.id}/${panel}/${toolsVersion}`}
                    translationControls={
                      <section
                        className="bc-book-progress"
                        aria-label={t("translateBatch")}
                      >
                        <h3>{t("translationProgress")}</h3>
                        <progress
                          aria-label={t("translationProgress")}
                          value={translatedCount}
                          max={Math.max(1, state.chapters.length)}
                        />
                        <p>
                          {t("chapterDone")}: {translatedCount} /{" "}
                          {state.chapters.length} (
                          {state.chapters.length
                            ? Math.round(
                                (translatedCount / state.chapters.length) * 100,
                              )
                            : 0}
                          %)
                        </p>
                        {(reviewedCount > 0 || failedCount > 0) && (
                          <p className="bc-hint">
                            {t("review")}: {reviewedCount} ·{" "}
                            {t("chapterFailed")}: {failedCount}
                          </p>
                        )}
                        <div className="bc-toolbar">
                          <label>
                            {t("batchCount")}
                            <input
                              type="number"
                              min="1"
                              max="4294967295"
                              step="1"
                              value={batchSize}
                              style={{ width: "6rem", marginLeft: "0.5rem" }}
                              onChange={(e) => {
                                const next = {
                                  ...batchSizes,
                                  [project.id]: e.target.value,
                                };
                                setBatchSizes(next);
                                localStorage.setItem(
                                  "bc.batchSizes",
                                  JSON.stringify(next),
                                );
                              }}
                            />
                          </label>
                          <button
                            disabled={
                              busy ||
                              translationRunning ||
                              !state.chapters.length ||
                              !validBatchSize
                            }
                            onClick={() => void act(() => translate(true))}
                          >
                            {t("translateBatch")}
                          </button>
                          <label className="bc-check">
                            <input
                              type="checkbox"
                              checked={force}
                              onChange={(e) => setForce(e.target.checked)}
                            />
                            {t("force")}
                          </label>
                          <label className="bc-check">
                            <input
                              type="checkbox"
                              checked={extractGlossary}
                              onChange={(e) =>
                                setExtractGlossary(e.target.checked)
                              }
                            />
                            {t("batchGlossary")}
                          </label>
                          <span className="bc-hint">{t("batchHint")}</span>
                        </div>
                      </section>
                    }
                    tool={panel}
                    registerFlush={registerFlush}
                    project={project}
                    chapters={state.chapters}
                    session={editor}
                    t={t}
                    run={run}
                    refresh={async () => {
                      await editor?.refresh();
                    }}
                  />
                ))
              )}
            </div>
          </main>
          {project?.kind === "book" && (
            <aside
              className="bc-assistant-panel"
              hidden={!showAssistant}
              aria-label={t("assistant")}
            >
              <Assistant
                key={project.id}
                projectId={project.id}
                chapterId={state.chapter?.chapter.id ?? null}
                t={t}
                registerFlush={registerAssistantFlush}
                onClose={() => setShowAssistant(false)}
                beforeWork={async () => {
                  await editorRef.current?.flush();
                  await toolFlush.current?.();
                }}
                refresh={async () => {
                  await editor?.refresh();
                  await workspace.refreshChapters();
                  setToolsVersion((v) => v + 1);
                }}
                onJob={async (job) => {
                  await jobs.refresh(job);
                  setShowJobs(true);
                }}
              />
            </aside>
          )}
        </div>
      )}
      {showJobs && (
        <JobPanel
          jobs={jobList}
          catalog={catalog}
          t={t}
          busy={busy}
          close={() => setShowJobs(false)}
          cancel={(job) =>
            void api
              .cancelJob(job)
              .then(() => jobs.refresh(job))
              .catch(setError)
          }
          resume={(job) =>
            void api
              .resumeJob(job)
              .then(() => jobs.refresh(job))
              .catch(setError)
          }
        />
      )}
      <footer className="bc-statusbar">
        <span>{project?.name ?? t("library")}</span>
        <span>{t("keyboard")}</span>
      </footer>
      {create && (
        <CreateProject
          t={t}
          lang={lang}
          onClose={() => setCreate(false)}
          onCreated={async (p, metadata) => {
            setCreate(false);
            await act(async () => {
              await reload();
              await activate(p.id);
              if (metadata) {
                const job = await api.startMetadata({ projectId: p.id });
                await jobs.refresh(job);
                setShowJobs(true);
              }
            });
          }}
        />
      )}
      {palette && (
        <CommandPalette
          commands={commands}
          close={() => setPalette(false)}
          t={t}
        />
      )}
      {settings && (
        <Settings
          project={project}
          t={t}
          lang={lang}
          setLang={(value) => {
            setLang(value);
            localStorage.setItem(LS_LANG, value);
          }}
          onClose={() => setSettings(false)}
        />
      )}
    </div>
  );
}
