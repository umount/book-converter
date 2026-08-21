import { useRef, useState } from "react";

/** Lines kept per project. Older ones scroll out of the console. */
const MAX_LINES = 300;

/**
 * The per-project console log.
 *
 * Its own hook, with no dependencies, on purpose. It used to live inside
 * `useTranslationJob`, which created a cycle: almost every other hook needs to
 * log, but the job hook is built last because it needs those hooks. The app
 * worked around that with a mutable ref that was filled in after mount and read
 * indirectly from everywhere. Owning the log separately removes the cycle, and
 * with it the indirection.
 *
 * Logs are keyed by project so a run continues writing to its own console while
 * another project is in the foreground. It takes no active project: that would
 * make it depend on the project list, which is exactly the ordering knot this
 * hook exists to avoid.
 */
export function useConsoleLog() {
  const [logsById, setLogsById] = useState<Record<string, string[]>>({});

  /** Append a line to a project's console. */
  const addLogTo = (id: string, m: string) => {
    if (!id) return;
    setLogsById((all) => ({ ...all, [id]: [...(all[id] ?? []).slice(-(MAX_LINES - 1)), m] }));
  };

  /** Lines of one project's console. */
  const linesOf = (id: string) => (id ? logsById[id] ?? [] : EMPTY);

  const clearLog = (id: string) => {
    if (id) setLogsById((all) => ({ ...all, [id]: [] }));
  };

  /** Drop a project's log entirely (the project was deleted). */
  const dropLog = (id: string) =>
    setLogsById((all) => { const n = { ...all }; delete n[id]; return n; });

  // Event listeners are registered once and must see the current writer.
  const addLogToRef = useRef(addLogTo);
  addLogToRef.current = addLogTo;

  return { linesOf, addLogTo, addLogToRef, clearLog, dropLog };
}

/** Stable empty array, so a project with no lines yet does not re-render on it. */
const EMPTY: string[] = [];
