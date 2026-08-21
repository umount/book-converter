import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { CallFn } from "../api";
import type { MenuId } from "../components/Menubar";
import {
  LS_ACTIVE, LS_PROJECTS, baseName, newId,
  type BookInfo, type Project, type ProjectSummary, type RefInfo, type ViewId,
} from "../types";

export type ProjectHelpers = {
  call: CallFn;
  t: (key: string, vars?: Record<string, string | number>) => string;
  setBusyFor: (id: string, msg: string | null) => void;
  setBusyById: React.Dispatch<React.SetStateAction<Record<string, string | null>>>;
  setError: (msg: string | null) => void;
  logError: (msg: string) => void;
  addLog: (m: string) => void;
  addLogTo: (id: string, m: string) => void;
  clearWorkspace: () => void;
  setBook: (b: BookInfo | null) => void;
  setRef: (r: RefInfo | null) => void;
  setPending: React.Dispatch<React.SetStateAction<Record<string, { old: string; new: string; kind: string }>>>;
  refreshDetails: () => Promise<void>;
  refreshProgressFor: (id: string) => Promise<void>;
  refreshProgress: () => Promise<void>;
  refreshGlossary: () => void | Promise<void>;
  translateTitle: () => Promise<void>;
  loadChapters: (projectId?: string) => Promise<void>;
  setChaptersLoading: (v: boolean) => void;
  openChapter: (idx: number) => void | Promise<void>;
  chapterIdxRef: MutableRefObject<number | null>;
  clearProjectJobState: (id: string) => void;
  setView: (v: ViewId) => void;
  setMenu: (m: MenuId | null) => void;
  errText: (raw: string) => string;
};

/** Projects list, active selection, open/save/delete, activate + localStorage. */
export function useProjects(helpersRef: MutableRefObject<ProjectHelpers>) {
  const [projects, setProjects] = useState<Project[]>(() => {
    try { return JSON.parse(localStorage.getItem(LS_PROJECTS) || "[]"); } catch { return []; }
  });
  const [active, setActive] = useState<number>(() => Number(localStorage.getItem(LS_ACTIVE) ?? -1));
  const activatingRef = useRef<string | null>(null);

  const activeProject = active >= 0 ? projects[active] : undefined;
  const activeId = activeProject?.id ?? "";

  useEffect(() => localStorage.setItem(LS_PROJECTS, JSON.stringify(projects)), [projects]);

  // localStorage is a cache of the list, not the record of it: the projects
  // themselves are directories on disk, each self-describing. Reconciling once
  // on startup means a cleared browser store, or a fresh machine pointed at the
  // same data directory, finds its projects instead of stranding them.
  const reconciled = useRef(false);
  useEffect(() => {
    if (reconciled.current) return;
    reconciled.current = true;
    (async () => {
      const h = helpersRef.current;
      const found = await h.call<ProjectSummary[]>("list_projects");
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
        if (recovered.length) {
          h.addLog(h.t("log.projectsRecovered", { n: recovered.length }));
        }
        return kept.length === current.length && recovered.length === 0
          ? current
          : [...kept, ...recovered];
      });
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => localStorage.setItem(LS_ACTIVE, String(active)), [active]);

  async function activateProject(p: Project) {
    const h = helpersRef.current;
    // Guard against the double invocation of the `[active]` effect (React
    // StrictMode in dev double-fires effects), which would open the book twice.
    if (activatingRef.current === p.id) return;
    activatingRef.current = p.id;
    h.setBusyFor(p.id, h.t("busy.opening", { name: p.name }));
    h.addLogTo(p.id, h.t("log.opening", { name: p.name }));
    h.setError(null);
    h.clearWorkspace();
    h.setPending({});
    // The explorer shows a preloader for the whole activation, not just the
    // list_chapters call, so it never flashes "no chapters" while opening.
    h.setChaptersLoading(true);
    // The frontend works only with the database: a project is always opened from
    // its own DB (the source file was parsed into it once, at add time).
    const info = await h.call<BookInfo>("open_project", { projectId: p.id }, { critical: true });
    if (info) {
      h.setBook(info);
      // Chapters can only be listed once open_project has registered the
      // session (it holds the DB path), hence after the await, not in parallel.
      await h.loadChapters(p.id);
      h.addLogTo(p.id, h.t("log.loaded", { name: p.name, n: info.total_chapters, format: info.format, encoding: info.encoding }));
      // Re-attach the reference for canon/style if its file is still available.
      if (p.refPath) {
        try {
          const r = await invoke<RefInfo>("load_reference", { projectId: p.id, path: p.refPath });
          h.setRef(r);
          h.addLogTo(p.id, h.t("log.reference", { n: r.max_covered ?? "?" }));
          if (r.imported > 0) h.addLogTo(p.id, h.t("log.refImported", { n: r.imported }));
        } catch { /* reference file gone (e.g. imported project) */ }
      }
      await h.refreshDetails();
      await h.refreshProgressFor(p.id);
      await h.refreshGlossary();
      void h.translateTitle();
    } else {
      h.setChaptersLoading(false); // open failed: stop the explorer preloader
    }
    h.setBusyFor(p.id, null);
  }

  useEffect(() => {
    const h = helpersRef.current;
    if (activeProject) void activateProject(activeProject);
    else { activatingRef.current = null; h.clearWorkspace(); h.setPending({}); }
    h.setView("overview");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Adding a book is a one-time import: the file is parsed into the project's DB
  // here, and from then on everything works from the DB. Each add (even the same
  // file) is a distinct, isolated project.
  async function openBook() {
    const h = helpersRef.current;
    h.setMenu(null);
    const path = await open({ filters: [{ name: "Book", extensions: ["txt", "fb2", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    const id = newId();
    h.setBusyFor(id, h.t("busy.opening", { name: baseName(path) }));
    h.setError(null);
    const info = await h.call<BookInfo>("load_source", { projectId: id, path }, { critical: true });
    if (!info) {
      h.setBusyFor(id, null);
      return; // parse failed (e.g. unreadable PDF): don't create a broken project
    }
    // Surface book-quality issues detected at load (encoding / chapter numbering).
    if (info.had_errors) h.addLogTo(id, h.t("log.warnEncoding", { encoding: info.encoding }));
    if (info.missing > 0 || info.duplicates > 0) h.addLogTo(id, h.t("log.warnChapters", { missing: info.missing, duplicates: info.duplicates }));
    // Leave busy set; activateProject (via setActive) will refresh and clear it.
    const next = [...projects, { id, path, name: baseName(path) }];
    setProjects(next); setActive(next.length - 1);
  }

  async function removeProject(idx: number) {
    const h = helpersRef.current;
    const p = projects[idx];
    if (p && !confirm(h.t("project.deleteConfirm", { name: p.name }))) return;
    if (p) {
      await h.call("delete_project", { projectId: p.id });
      h.clearProjectJobState(p.id);
      h.setBusyById((all) => { const n = { ...all }; delete n[p.id]; return n; });
    }
    setProjects(projects.filter((_, i) => i !== idx));
    setActive(idx === active ? -1 : active > idx ? active - 1 : active);
  }

  async function openReference() {
    const h = helpersRef.current;
    h.setMenu(null);
    if (!activeProject) return;
    const path = await open({ filters: [{ name: "Reference", extensions: ["fb2", "txt", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    const name = baseName(path);
    h.setBusyFor(activeId, h.t("busy.loadingReference"));
    h.addLog(h.t("log.loadingReference", { name }));
    const info = await h.call<RefInfo>("load_reference", { projectId: activeId, path });
    h.setBusyFor(activeId, null);
    if (info) {
      h.setRef(info);
      setProjects((ps) => ps.map((p) => (p.id === activeId ? { ...p, refPath: path } : p)));
      h.addLog(h.t("log.reference", { n: info.max_covered ?? "?" }));
      if (info.imported > 0) h.addLog(h.t("log.refImported", { n: info.imported }));
      void h.refreshDetails(); void h.refreshProgress(); void h.loadChapters();
      const i = h.chapterIdxRef.current; if (i != null) void h.openChapter(i);
    }
  }

  // Save the active project to a portable .bcproj archive.
  async function saveProject() {
    const h = helpersRef.current;
    h.setMenu(null);
    if (!activeProject) return;
    const out = await save({
      defaultPath: `${activeProject.name}.bcproj`,
      filters: [{ name: "book-converter project", extensions: ["bcproj"] }],
    });
    if (typeof out !== "string") return;
    const path = out.toLowerCase().endsWith(".bcproj") ? out : `${out}.bcproj`;
    h.setBusyFor(activeId, h.t("busy.savingProject"));
    try {
      await invoke("export_project", { projectId: activeId, outPath: path });
      h.addLog(h.t("log.projectSaved", { path }));
    } catch (e) { h.logError(String(e)); }
    finally { h.setBusyFor(activeId, null); }
  }

  // Open a .bcproj archive as a new isolated project.
  async function openProjectArchive() {
    const h = helpersRef.current;
    h.setMenu(null);
    const arch = await open({ filters: [{ name: "book-converter project", extensions: ["bcproj"] }] });
    if (typeof arch !== "string") return;
    const id = newId();
    h.setBusyFor(id, h.t("busy.openingProject"));
    try {
      const res = await invoke<{ name: string; source_path: string }>("import_project", { projectId: id, archivePath: arch });
      // Leave busy set; activateProject (via setActive) will refresh and clear it.
      const next = [...projects, { id, path: res.source_path, name: res.name }];
      setProjects(next); setActive(next.length - 1);
    } catch (e) {
      h.logError(String(e));
      h.setBusyFor(id, null);
    }
  }

  async function generateSummary() {
    const h = helpersRef.current;
    h.setBusyFor(activeId, h.t("busy.generatingSummary"));
    try {
      await invoke<string>("generate_summary", { projectId: activeId });
      h.addLog(h.t("log.summaryGenerated"));
      void h.refreshDetails();
    } catch (e) {
      // Not finding the book is an expected outcome, not a critical error: log it.
      const msg = String(e);
      h.addLog(msg.includes("book_not_found") ? h.t("log.summaryNotFound") : h.t("log.error", { msg: h.errText(msg) }));
    } finally {
      h.setBusyFor(activeId, null);
    }
  }

  async function exportAs(fmt: "fb2" | "epub" | "pdf" | "txt") {
    const h = helpersRef.current;
    h.setMenu(null);
    const outPath = await save({ defaultPath: `book.${fmt}`, filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }] });
    if (typeof outPath !== "string") return;
    const path = outPath.toLowerCase().endsWith(`.${fmt}`) ? outPath : `${outPath}.${fmt}`;
    h.setBusyFor(activeId, h.t("busy.exporting", { fmt: fmt.toUpperCase() }));
    const p = await h.call<string>("export_book", { projectId: activeId, outPath: path });
    h.setBusyFor(activeId, null);
    if (p) h.addLog(h.t("log.exported", { path: p }));
  }

  return {
    projects, setProjects,
    active, setActive,
    activeProject, activeId,
    openBook, removeProject, openReference,
    saveProject, openProjectArchive,
    generateSummary, exportAs,
  };
}
