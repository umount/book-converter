import { useEffect, useRef, useState } from "react";
import type { CallFn } from "../api";
import { LS_ACTIVE, LS_PROJECTS, type Project, type ProjectSummary } from "../types";

type Opts = {
  call: CallFn;
};

/**
 * The list of projects and which one is active.
 *
 * Deliberately holds no behaviour that needs the book, glossary or job hooks:
 * those are built from `activeId`, so anything they touch has to run after
 * them. Keeping selection state here and the actions in `useProjectActions`
 * lets both sides take their dependencies directly, instead of reaching each
 * other through a mutable ref filled in after render.
 */
export function useProjectList({ call }: Opts) {
  const [projects, setProjects] = useState<Project[]>(() => {
    try { return JSON.parse(localStorage.getItem(LS_PROJECTS) || "[]"); } catch { return []; }
  });
  const [active, setActive] = useState<number>(() => Number(localStorage.getItem(LS_ACTIVE) ?? -1));
  /** How many projects the startup reconcile found on disk but not in the list.
   *  Reported rather than logged here: this hook has no console to write to,
   *  and giving it one would recreate the dependency it exists without. */
  const [recovered, setRecovered] = useState(0);

  const activeProject = active >= 0 ? projects[active] : undefined;
  const activeId = activeProject?.id ?? "";

  useEffect(() => localStorage.setItem(LS_PROJECTS, JSON.stringify(projects)), [projects]);
  useEffect(() => localStorage.setItem(LS_ACTIVE, String(active)), [active]);

  // localStorage is a cache of the list, not the record of it: the projects
  // themselves are directories on disk, each self-describing. Reconciling once
  // on startup means a cleared browser store, or a fresh machine pointed at the
  // same data directory, finds its projects instead of stranding them.
  const reconciled = useRef(false);
  useEffect(() => {
    if (reconciled.current) return;
    reconciled.current = true;
    (async () => {
      const found = await call<ProjectSummary[]>("list_projects");
      if (!found) return;
      setProjects((current) => {
        const onDisk = new Map(found.map((p) => [p.id, p]));
        // Drop rows whose data is gone (deleted outside the app), keep the rest
        // in their existing order so the active index stays meaningful.
        const kept = current.filter((p) => onDisk.has(p.id));
        const known = new Set(kept.map((p) => p.id));
        const recovered = found
          .filter((p) => !known.has(p.id))
          .map((p) => ({
            id: p.id,
            path: p.source_path,
            name: p.name,
            ...(p.ref_path ? { refPath: p.ref_path } : {}),
          }));
        if (recovered.length) setRecovered(recovered.length);
        return kept.length === current.length && recovered.length === 0
          ? current
          : [...kept, ...recovered];
      });
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Append a project row and make it active. */
  function addProject(p: Project) {
    setProjects((ps) => {
      const next = [...ps, p];
      setActive(next.length - 1);
      return next;
    });
  }

  /** Remove a row, keeping `active` pointing at the same project it did. */
  function removeAt(idx: number) {
    setProjects((ps) => ps.filter((_, i) => i !== idx));
    setActive(idx === active ? -1 : active > idx ? active - 1 : active);
  }

  /** Record the reference file attached to a project. */
  function setRefPath(id: string, refPath: string) {
    setProjects((ps) => ps.map((p) => (p.id === id ? { ...p, refPath } : p)));
  }

  return {
    projects, setProjects,
    active, setActive,
    activeProject, activeId,
    recovered,
    addProject, removeAt, setRefPath,
  };
}
