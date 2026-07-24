import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CallFn } from "../api";
import type { Term } from "../types";

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

/** Glossary state, CRUD, pending renames, and retarget. */
export function useGlossary({
  call, activeId, setBusyFor, setError, addLog, logError, t,
}: Opts) {
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [glossaryQuery, setGlossaryQuery] = useState("");
  const [newTerm, setNewTerm] = useState<{ source: string; target: string; kind: string }>({
    source: "", target: "", kind: "person",
  });
  // Renames whose new rendering has been saved to the glossary but not yet
  // propagated into the existing translation. Keyed by source; `old` is the
  // rendering still present in the translated text. Drives the global button.
  const [pending, setPending] = useState<Pending>({});
  const pendingCount = Object.keys(pending).length;

  async function refreshGlossary() {
    const g = await call<Term[]>("get_glossary", { projectId: activeId });
    if (g) setGlossary(g);
  }

  // Term edits auto-save. Changing the rendering also records a pending rename so
  // the global "Update translation" button can later propagate it into the text.
  async function saveTermField(term: Term, patch: Partial<Term>) {
    await call("update_term", { projectId: activeId, term: { ...term, ...patch, pinned: true } });
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
    refreshGlossary();
  }
  async function renameTerm(term: Term, source: string) {
    await call("update_term", { projectId: activeId, term: { ...term, source, pinned: true } });
    await call("delete_term", { projectId: activeId, source: term.source });
    refreshGlossary();
  }
  async function addTerm() {
    const source = newTerm.source.trim(), target = newTerm.target.trim();
    if (!source || !target) return;
    await call("update_term", {
      projectId: activeId,
      term: { source, target, kind: newTerm.kind, frequency: 1, pinned: true },
    });
    setNewTerm({ source: "", target: "", kind: "person" });
    refreshGlossary();
  }

  const filteredGlossary = useMemo(() => {
    const q = glossaryQuery.trim().toLowerCase();
    return q
      ? glossary.filter((term) => term.source.toLowerCase().includes(q) || term.target.toLowerCase().includes(q))
      : glossary;
  }, [glossary, glossaryQuery]);
  return {
    glossary, setGlossary,
    glossaryQuery, setGlossaryQuery,
    newTerm, setNewTerm,
    pending, setPending, pendingCount,
    filteredGlossary,
    refreshGlossary,
    editTarget, editKind, updateTranslation,
    deleteTerm, renameTerm, addTerm,
  };
}
