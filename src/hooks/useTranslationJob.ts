import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CallFn } from "../api";
import type { BookInfo, Progress } from "../types";

/** Format seconds as `Xm Ys` / `Xh Ym` for ETA display. */
function formatEta(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m < 60) return r > 0 ? `${m}m ${r}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return rm > 0 ? `${h}h ${rm}m` : `${h}h`;
}

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
  /** Book chapter number (`第N章`) for a reading-order index, when known. */
  chapterNumberOf: (idx: number) => number | null;
  loadChapters: () => void | Promise<void>;
  /** Apply a saved manual edit locally (no refetch, keeps the editor mounted). */
  applyChapterEdit: (idx: number, title: string, body: string) => void;
  errText: (raw: string) => string;
  limit: number | "";
};

/** Progress/logs, translation job controls, and backend event listeners. */
export function useTranslationJob({
  call, activeId, book, t, tRef, activeIdRef, chapterIdxRef,
  setBusyFor, setError, setPending, refreshGlossary, openChapter, loadChapters,
  chapterNumberOf, applyChapterEdit, errText, limit,
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
      listen<Progress>("progress", (e) => {
        const p = e.payload;
        setProgressForRef.current(p.project, p);
        if (p.phase === "start") {
          const n = p.job_total || 0;
          addLogToRef.current(p.project, tr("log.jobProgressStart", { n }));
          if (p.eta_secs != null && p.eta_secs > 0) {
            addLogToRef.current(p.project, tr("log.eta", { eta: formatEta(p.eta_secs) }));
          }
        } else if (p.phase === "chapter_start") {
          const title = (p.current_title || "").slice(0, 60);
          const n = p.current_number ?? p.current_idx ?? "?";
          addLogToRef.current(
            p.project,
            tr("log.chapterProgress", {
              done: (p.job_done ?? 0) + 1,
              total: p.job_total || "?",
              n,
              title,
            }),
          );
        } else if (p.phase === "chapter_done") {
          const secs = p.last_ms != null ? Math.max(1, Math.round(p.last_ms / 1000)) : null;
          const n = p.current_number ?? p.current_idx ?? "?";
          const parts = [
            tr("log.chapterDone", {
              n,
              took: secs != null ? tr("log.tookSecs", { n: secs }) : "",
            }),
          ];
          if (p.eta_secs != null && (p.job_done ?? 0) < (p.job_total ?? 0)) {
            parts.push(tr("log.eta", { eta: formatEta(p.eta_secs) }));
          }
          addLogToRef.current(p.project, parts.filter(Boolean).join(" · "));
          // Live-update the explorer tree + tabs (title/status) as each chapter
          // finishes, and refresh the open chapter if it is the one just done.
          if (isActive(p.project)) {
            void loadChaptersRef.current();
            // The run extracts terms from every chapter and writes them before
            // moving on, so the glossary the user is looking at must follow
            // along. Waiting for `done` meant a run of hundreds of chapters
            // showed a stale glossary the whole way through.
            void refreshGlossaryRef.current();
            if (p.current_idx != null && p.current_idx === chapterIdxRef.current) {
              void openChapterRef.current(p.current_idx);
            }
          }
        }
      }),
      listen<{ project: string }>("done", (e) => {
        const id = e.payload.project;
        addLogToRef.current(id, tr("log.runFinished"));
        // Clear running immediately so Edit / other actions unlock even if
        // get_progress is slow or fails.
        setProgressById((all) => {
          const prev = all[id];
          return prev ? { ...all, [id]: { ...prev, running: false } } : all;
        });
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
        setProgressById((all) => {
          const prev = all[project];
          return prev ? { ...all, [project]: { ...prev, running: false } } : all;
        });
        void refreshProgressForRef.current(project);
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
    addLog(t("log.bootstrapping", { n: sample }));
    const n = await call<number>("bootstrap_glossary", { projectId: activeId, sample }, { critical: true });
    setBusyFor(activeId, null);
    if (n !== undefined) { addLog(t("log.bootstrapped", { n })); void refreshGlossary(); }
  }

  /** Pull terms from already-translated chapters (start or end of the done range). */
  async function harvestGlossary(fromEnd: boolean) {
    if (progress?.running) return;
    setBusyFor(activeId, t("busy.harvesting", { n: sample, where: fromEnd ? t("glossary.harvestEnd") : t("glossary.harvestStart") }));
    addLog(t("log.harvesting", { n: sample, where: fromEnd ? t("glossary.harvestEnd") : t("glossary.harvestStart") }));
    const n = await call<number>(
      "harvest_glossary",
      { projectId: activeId, sample, fromEnd },
      { critical: true },
    );
    setBusyFor(activeId, null);
    if (n !== undefined) { addLog(t("log.harvested", { n })); void refreshGlossary(); }
  }

  async function start() {
    const lim = limit === "" ? null : Number(limit);
    const ok = await call("start_translation", { projectId: activeId, limit: lim });
    // invoke Ok(()) → null; makeCall returns undefined only on failure
    if (ok === undefined) return;
    addLog(t("log.started", { suffix: lim ? t("log.startedNext", { n: lim }) : "" }));
    // Optimistic: mark running immediately so the bar/Pause enable before first chapter event.
    if (progress) {
      setProgressFor(activeId, { ...progress, running: true });
    } else {
      void refreshProgress();
    }
  }
  async function pause() {
    await call("pause_translation", { projectId: activeId });
    addLog(t("log.pauseRequested"));
  }

  async function translateChapter(idx: number) {
    if (progress?.running) return;
    setBusyFor(activeId, t("busy.translatingChapter"));
    addLog(t("log.chapterStarted", { n: chapterNumberOf(idx) ?? idx }));
    await call("translate_chapter", { projectId: activeId, index: idx });
  }

  async function saveChapterPrompt(idx: number, prompt: string) {
    await call("set_chapter_prompt", { projectId: activeId, index: idx, prompt });
    addLog(t("log.chapterPromptSaved"));
    await openChapter(idx);
  }

  async function saveChapterContext(idx: number, summary: string, prevTail: string) {
    await call("set_chapter_context", {
      projectId: activeId,
      index: idx,
      summary,
      prevTail,
    });
    addLog(t("log.chapterContextSaved"));
    await openChapter(idx);
  }

  /** Persist the chapter instruction, then (re)translate that chapter with it. */
  async function retranslateWithPrompt(idx: number, prompt: string) {
    if (progress?.running) return;
    await call("set_chapter_prompt", { projectId: activeId, index: idx, prompt });
    setBusyFor(activeId, t("busy.translatingChapter"));
    addLog(t("log.chapterRetranslate", { n: chapterNumberOf(idx) ?? idx }));
    await call("translate_chapter", { projectId: activeId, index: idx });
  }

  /**
   * Persist an edit made directly in the translation pane. Kept deliberately
   * cheap (one IPC call, then a local patch): it runs on every autosave while
   * the user is typing, so it must not reload the chapter or the whole tree.
   */
  async function saveChapterTranslation(idx: number, title: string, body: string) {
    await call("update_chapter_translation", {
      projectId: activeId,
      index: idx,
      translatedTitle: title,
      translated: body,
    });
    applyChapterEdit(idx, title, body);
  }

  // Reset chapters to pending for a fresh run with the current glossary. `pos` is
  // the book chapter number from the title (e.g. 523); null means the whole book.
  async function reTranslate(pos: number | null) {
    if (progress?.running) return;
    const total = progress?.total ?? book?.total_chapters ?? 0;
    const msg = pos == null
      ? t("translate.retranslateAllConfirm", { n: total })
      : t("translate.retranslateFromConfirm", { from: pos });
    if (!confirm(msg)) return;
    const fromNumber = pos == null ? null : Math.max(1, pos);
    const n = await call<number>("reset_translation", { projectId: activeId, fromNumber });
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
    bootstrap, harvestGlossary, start, pause, reTranslate,
    translateChapter, saveChapterTranslation,
    saveChapterPrompt, saveChapterContext, retranslateWithPrompt,
    clearProjectJobState,
  };
}
