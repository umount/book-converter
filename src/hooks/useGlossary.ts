import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CallFn } from "../api";
import type { GlossaryPage, Term } from "../types";

type Pending = Record<string, { old: string; new: string; kind: string }>;

type Opts = {
  call: CallFn;
  activeId: string;
  setBusyFor: (id: string, msg: string | null) => void;
  setError: (msg: string | null) => void;
  addLog: (m: string) => void;
  logError: (msg: string) => void;
  t: (key: string, vars?: Record<string, string | number>) => string;
};

/** Rows fetched per request. Big enough that scrolling rarely waits, small
 *  enough that a 10K-term glossary is never shipped over IPC at once. */
const PAGE = 200;
/** Delay before a filter keystroke becomes a query. */
const FILTER_DEBOUNCE_MS = 200;

/**
 * Glossary state, CRUD, pending renames, and retarget.
 *
 * The glossary of a long book runs to tens of thousands of terms, so the list
 * is never held in full: the backend filters, orders and windows it
 * (`get_glossary_page`) and this hook keeps the pages scrolled through so far.
 * `total` is the size of the current filter, not of what is loaded.
 */
export function useGlossary({
  call, activeId, setBusyFor, setError, addLog, logError, t,
}: Opts) {
  const [terms, setTerms] = useState<Term[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [glossaryQuery, setGlossaryQuery] = useState("");
  const [kindFilter, setKindFilter] = useState<string>("all");
  const [newTerm, setNewTerm] = useState<{ source: string; target: string; kind: string }>({
    source: "", target: "", kind: "person",
  });
  // Renames whose new rendering has been saved to the glossary but not yet
  // propagated into the existing translation. Keyed by source; `old` is the
  // rendering still present in the translated text. Drives the global button.
  const [pending, setPending] = useState<Pending>({});
  const pendingCount = Object.keys(pending).length;

  // Live refs so a page that resolves late can tell whether it is still wanted.
  const requestRef = useRef(0);
  const filterRef = useRef({ query: "", kind: "all", projectId: "" });
  filterRef.current = { query: glossaryQuery.trim(), kind: kindFilter, projectId: activeId };

  async function loadPage(offset: number) {
    if (!activeId) {
      setTerms([]);
      setTotal(0);
      return;
    }
    const { query, kind, projectId } = filterRef.current;
    const token = ++requestRef.current;
    setLoading(true);
    try {
      const page = await call<GlossaryPage>("get_glossary_page", {
        projectId,
        query: query || null,
        kind: kind === "all" ? null : kind,
        offset,
        limit: PAGE,
      });
      // A newer request (or a project switch) has superseded this one.
      if (token !== requestRef.current || filterRef.current.projectId !== projectId) return;
      if (!page) return;
      setTotal(page.total);
      setTerms((prev) => (offset === 0 ? page.terms : [...prev, ...page.terms]));
    } finally {
      if (token === requestRef.current) setLoading(false);
    }
  }

  /** Reload from the top, keeping the current filter. */
  async function refreshGlossary() {
    await loadPage(0);
  }

  /** Fetch the next window; a no-op once everything matching is loaded. */
  function loadMore() {
    if (loading || terms.length >= total) return;
    void loadPage(terms.length);
  }

  // A changed filter is a new query, so it restarts at the top. Debounced so
  // typing does not fire a request per keystroke.
  useEffect(() => {
    const id = setTimeout(() => void loadPage(0), FILTER_DEBOUNCE_MS);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [glossaryQuery, kindFilter, activeId]);

  // Term edits auto-save. Changing the rendering also records a pending rename so
  // the global "Update translation" button can later propagate it into the text.
  async function saveTermField(term: Term, patch: Partial<Term>) {
    await call("update_term", { projectId: activeId, term: { ...term, ...patch, pinned: true } });
    // Patch the loaded row in place: a refetch here would reset the scroll
    // position and re-order the row out from under the cursor.
    setTerms((all) => all.map((x) => (x.source === term.source ? { ...x, ...patch, pinned: true } : x)));
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
    setBusyFor(activeId, t("busy.updatingTranslation"));
    setError(null);
    addLog(t("log.retargetStart", { n: list.length, list: summary }));
    // Progress/finish are driven by retarget_progress / retarget_done events;
    // only a synchronous rejection needs to clear the busy state here.
    try {
      await invoke("retarget_terms", {
        projectId: activeId,
        changes: list.map((c) => ({ old_target: c.old, new_target: c.new, kind: c.kind })),
      });
    } catch (e) {
      setBusyFor(activeId, null);
      logError(String(e));
    }
  }
  async function deleteTerm(term: Term) {
    await call("delete_term", { projectId: activeId, source: term.source });
    setPending((p) => { const n = { ...p }; delete n[term.source]; return n; });
    setTerms((all) => all.filter((x) => x.source !== term.source));
    setTotal((n) => Math.max(0, n - 1));
  }
  async function renameTerm(term: Term, source: string) {
    await call("update_term", { projectId: activeId, term: { ...term, source, pinned: true } });
    await call("delete_term", { projectId: activeId, source: term.source });
    void refreshGlossary();
  }
  async function addTerm() {
    const source = newTerm.source.trim(), target = newTerm.target.trim();
    if (!source || !target) return;
    await call("update_term", {
      projectId: activeId,
      term: { source, target, kind: newTerm.kind, frequency: 1, pinned: true },
    });
    setNewTerm({ source: "", target: "", kind: "person" });
    void refreshGlossary();
  }

  return {
    terms, total, loading,
    glossaryQuery, setGlossaryQuery,
    kindFilter, setKindFilter,
    newTerm, setNewTerm,
    pending, setPending, pendingCount,
    refreshGlossary, loadMore,
    editTarget, editKind, updateTranslation,
    deleteTerm, renameTerm, addTerm,
  };
}
