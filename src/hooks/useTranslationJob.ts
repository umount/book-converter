import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CallFn } from "../api";
import type { BookInfo, Progress } from "../types";

type TranslateOpts = {
  call: CallFn;
  activeId: string;
  book: BookInfo | null;
  t: (key: string, vars?: Record<string, string | number>) => string;
  /** Live translator (event listeners read via ref). */
  tRef: MutableRefObject<(key: string, vars?: Record<string, string | number>) => string>;
  activeIdRef: MutableRefObject<string>;
  chapterIdxRef: MutableRefObject<number | null>;
  setBusyFor: (id: string, msg: string | null) => void;
  setError: (msg: string | null) => void;
  setPending: React.Dispatch<React.SetStateAction<Record<string, { old: string; new: string; kind: string }>>>;
  refreshGlossary: () => void | Promise<void>;
  openChapter: (idx: number) => void | Promise<void>;
  loadChapters: () => void | Promise<void>;
  errText: (raw: string) => string;
  limit: number | "";
};

/** Progress/logs, translation job controls, and backend event listeners. */
export function useTranslationJob({
  call, activeId, book, t, tRef, activeIdRef, chapterIdxRef,
  setBusyFor, setError, setPending, refreshGlossary, openChapter, loadChapters, errText, limit,
}: TranslateOpts) {
  // Progress and console log are per project (keyed by id) so background/parallel
  // runs keep updating even while another project is in the foreground.
  const [progressById, setProgressById] = useState<Record<string, Progress>>({});
  const [logsById, setLogsById] = useState<Record<string, string[]>>({});
  const [sample, setSample] = useState(30);
  const [reFrom, setReFrom] = useState<number>(1);

  const log = activeId ? logsById[activeId] ?? [] : [];
  const progress = activeId ? progressById[activeId] ?? null : null;

  // Append a log line to a project's console (defaults to the active project).
  const addLogTo = (id: string, m: string) =>
    setLogsById((all) => ({ ...all, [id]: [...(all[id] ?? []).slice(-300), m] }));
  const addLog = (m: string) => { if (activeId) addLogTo(activeId, m); };
  const setProgressFor = (id: string, p: Progress) =>
    setProgressById((all) => ({ ...all, [id]: p }));

  async function refreshProgressFor(id: string) {
    if (!id) return;
    const p = await call<Progress>("get_progress", { projectId: id });
    if (p) setProgressFor(id, p);
  }
  async function refreshProgress() { await refreshProgressFor(activeId); }

  // Stable refs for callbacks used inside once-registered listeners.
  const refreshProgressForRef = useRef(refreshProgressFor);
  refreshProgressForRef.current = refreshProgressFor;
  const refreshGlossaryRef = useRef(refreshGlossary);
  refreshGlossaryRef.current = refreshGlossary;
  const openChapterRef = useRef(openChapter);
  openChapterRef.current = openChapter;
  const loadChaptersRef = useRef(loadChapters);
  loadChaptersRef.current = loadChapters;
  const setBusyForRef = useRef(setBusyFor);
  setBusyForRef.current = setBusyFor;
  const setErrorRef = useRef(setError);
  setErrorRef.current = setError;
  const setPendingRef = useRef(setPending);
  setPendingRef.current = setPending;
  const errTextRef = useRef(errText);
  errTextRef.current = errText;
  const addLogToRef = useRef(addLogTo);
  addLogToRef.current = addLogTo;
  const setProgressForRef = useRef(setProgressFor);
  setProgressForRef.current = setProgressFor;

  useEffect(() => {
    const tr = (key: string, vars?: Record<string, string | number>) => tRef.current(key, vars);
    const isActive = (id: string) => id === activeIdRef.current;
    const unsubs = [
      listen<Progress>("progress", (e) => setProgressForRef.current(e.payload.project, e.payload)),
      listen<{ project: string }>("done", (e) => {
        const id = e.payload.project;
        addLogToRef.current(id, tr("log.runFinished"));
        void refreshProgressForRef.current(id);
        setBusyForRef.current(id, null);
        if (isActive(id)) {
          void refreshGlossaryRef.current();
          void loadChaptersRef.current();
          const i = chapterIdxRef.current;
          if (i != null) void openChapterRef.current(i);
        }
      }),
      listen<{ project: string; message: string }>("job_error", (e) => {
        const { project, message } = e.payload;
        addLogToRef.current(project, tr("log.error", { msg: errTextRef.current(message) }));
        // Clear busy for the event's project even if another project is active.
        setBusyForRef.current(project, null);
        if (isActive(project)) setErrorRef.current(errTextRef.current(message));
      }),
      listen<{ project: string; done: number; total: number; title: string; changed: boolean }>("retarget_progress", (e) => {
        const { project, done, total, title, changed } = e.payload;
        addLogToRef.current(project, tr("log.retargetItem", { done, total, mark: changed ? "✓" : "·", title }));
        setBusyForRef.current(project, tr("busy.updatingTranslationN", { done, total }));
      }),
      listen<{ project: string; message: string }>("retarget_warn", (e) =>
        addLogToRef.current(e.payload.project, tr("log.retargetWarn", { msg: e.payload.message }))),
      listen<{ project: string; changed: number }>("retarget_done", (e) => {
        const { project, changed } = e.payload;
        addLogToRef.current(project, tr("log.renamed", { n: changed }));
        void refreshProgressForRef.current(project);
        setBusyForRef.current(project, null);
        if (isActive(project)) {
          setPendingRef.current({});
          const i = chapterIdxRef.current;
          if (i != null) void openChapterRef.current(i);
        }
      }),
    ];
    return () => unsubs.forEach((u) => u.then((f) => f()));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function bootstrap() {
    setBusyFor(activeId, t("busy.bootstrapping", { n: sample }));
    const n = await call<number>("bootstrap_glossary", { projectId: activeId, sample }, { critical: true });
    setBusyFor(activeId, null);
    if (n !== undefined) { addLog(t("log.bootstrapped", { n })); void refreshGlossary(); }
  }

  async function start() {
    const lim = limit === "" ? null : Number(limit);
    await call("start_translation", { projectId: activeId, limit: lim });
    addLog(t("log.started", { suffix: lim ? t("log.startedNext", { n: lim }) : "" }));
  }
  async function pause() {
    await call("pause_translation", { projectId: activeId });
    addLog(t("log.pauseRequested"));
  }

  async function translateChapter(idx: number) {
    if (progress?.running) return;
    setBusyFor(activeId, t("busy.translatingChapter"));
    addLog(t("log.chapterStarted", { n: idx }));
    await call("translate_chapter", { projectId: activeId, index: idx });
  }

  async function saveChapterTranslation(idx: number, title: string, body: string) {
    await call("update_chapter_translation", {
      projectId: activeId,
      index: idx,
      translatedTitle: title,
      translated: body,
    });
    addLog(t("log.chapterSaved"));
    await loadChapters();
    await openChapter(idx);
    await refreshProgress();
  }

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
    const n = await call<number>("reset_translation", { projectId: activeId, fromIndex });
    if (n !== undefined) { addLog(t("log.reset", { n })); await refreshProgress(); }
  }

  function clearProjectJobState(id: string) {
    setLogsById((all) => { const n = { ...all }; delete n[id]; return n; });
    setProgressById((all) => { const n = { ...all }; delete n[id]; return n; });
  }

  return {
    progressById, setProgressById,
    logsById, setLogsById,
    log,
    progress,
    sample, setSample,
    reFrom, setReFrom,
    addLog, addLogTo,
    setProgressFor,
    refreshProgressFor, refreshProgress,
    bootstrap, start, pause, reTranslate,
    translateChapter, saveChapterTranslation,
    clearProjectJobState,
  };
}
