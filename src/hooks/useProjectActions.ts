import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { CallFn } from "../api";
import type { MenuId } from "../components/Menubar";
import { baseName, newId, type BookInfo, type Project, type RefInfo, type ViewId } from "../types";
import type { useBookWorkspace } from "./useBookWorkspace";
import type { useGlossary } from "./useGlossary";
import type { useProjectList } from "./useProjectList";
import type { useTranslationJob } from "./useTranslationJob";

type Opts = {
  call: CallFn;
  t: (key: string, vars?: Record<string, string | number>) => string;
  errText: (raw: string) => string;
  addLog: (m: string) => void;
  addLogTo: (id: string, m: string) => void;
  logError: (msg: string) => void;
  setBusyFor: (id: string, msg: string | null) => void;
  setBusyById: React.Dispatch<React.SetStateAction<Record<string, string | null>>>;
  setError: (msg: string | null) => void;
  setView: (v: ViewId) => void;
  setMenu: (m: MenuId | null) => void;
  list: ReturnType<typeof useProjectList>;
  book: ReturnType<typeof useBookWorkspace>;
  glossary: ReturnType<typeof useGlossary>;
  job: ReturnType<typeof useTranslationJob>;
};

/**
 * Everything that *does* something to a project: opening a book, activating
 * one, attaching a reference, saving and exporting.
 *
 * Runs after the book, glossary and job hooks, so it can call them directly.
 * These actions used to live beside the project list, which is built first
 * because everything else keys off `activeId`, and reached the other hooks
 * through a ref of twenty-five callbacks assigned during render.
 */
export function useProjectActions({
  call, t, errText, addLog, addLogTo, logError,
  setBusyFor, setBusyById, setError, setView, setMenu,
  list, book, glossary, job,
}: Opts) {
  const { projects, active, activeProject, activeId, addProject, removeAt, setRefPath } = list;

  const activatingRef = useRef<string | null>(null);

  /**
   * Open a project and load everything that belongs to it.
   *
   * The order is not incidental: `open_project` is what registers the backend
   * session holding the DB path, and every other per-project command resolves
   * its database through that session. Anything fired before it resolves fails
   * with `no_source`. See docs/PROJECT_ISOLATION.md.
   */
  async function activateProject(p: Project) {
    // React StrictMode double-fires effects in dev, which would open twice.
    if (activatingRef.current === p.id) return;
    activatingRef.current = p.id;
    setBusyFor(p.id, t("busy.opening", { name: p.name }));
    addLogTo(p.id, t("log.opening", { name: p.name }));
    setError(null);
    book.clearWorkspace();
    glossary.setPending({});
    // The explorer shows a preloader for the whole activation, not just the
    // list_chapters call, so it never flashes "no chapters" while opening.
    book.setChaptersLoading(true);
    // The frontend works only with the database: a project is always opened
    // from its own DB (the source file was parsed into it once, at add time).
    const info = await call<BookInfo>("open_project", { projectId: p.id }, { critical: true });
    if (!info) {
      book.setChaptersLoading(false); // open failed: stop the explorer preloader
      setBusyFor(p.id, null);
      return;
    }
    book.setBook(info);
    await book.loadChapters(p.id);
    addLogTo(p.id, t("log.loaded", {
      name: p.name, n: info.total_chapters, format: info.format, encoding: info.encoding,
    }));
    // Re-attach the reference for canon/style if its file is still available.
    if (p.refPath) {
      try {
        const r = await invoke<RefInfo>("load_reference", { projectId: p.id, path: p.refPath });
        book.setRef(r);
        addLogTo(p.id, t("log.reference", { n: r.max_covered ?? "?" }));
        if (r.imported > 0) addLogTo(p.id, t("log.refImported", { n: r.imported }));
      } catch { /* reference file gone (e.g. imported project) */ }
    }
    await book.refreshDetails();
    await job.refreshProgressFor(p.id);
    await glossary.refreshGlossary();
    void book.translateTitle();
    setBusyFor(p.id, null);
  }

  useEffect(() => {
    if (activeProject) void activateProject(activeProject);
    else { activatingRef.current = null; book.clearWorkspace(); glossary.setPending({}); }
    setView("overview");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  // Adding a book is a one-time import: the file is parsed into the project's DB
  // here, and from then on everything works from the DB. Each add (even the same
  // file) is a distinct, isolated project.
  async function openBook() {
    setMenu(null);
    const path = await open({ filters: [{ name: "Book", extensions: ["txt", "fb2", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    const id = newId();
    setBusyFor(id, t("busy.opening", { name: baseName(path) }));
    setError(null);
    const info = await call<BookInfo>("load_source", { projectId: id, path }, { critical: true });
    if (!info) {
      setBusyFor(id, null);
      return; // parse failed (e.g. unreadable PDF): don't create a broken project
    }
    if (info.had_errors) addLogTo(id, t("log.warnEncoding", { encoding: info.encoding }));
    if (info.missing > 0 || info.duplicates > 0) {
      addLogTo(id, t("log.warnChapters", { missing: info.missing, duplicates: info.duplicates }));
    }
    // Leave busy set; activation (via setActive) will refresh and clear it.
    addProject({ id, path, name: baseName(path) });
  }

  async function removeProject(idx: number) {
    const p = projects[idx];
    if (p && !confirm(t("project.deleteConfirm", { name: p.name }))) return;
    if (p) {
      await call("delete_project", { projectId: p.id });
      job.clearProjectJobState(p.id);
      setBusyById((all) => { const n = { ...all }; delete n[p.id]; return n; });
    }
    removeAt(idx);
  }

  async function openReference() {
    setMenu(null);
    if (!activeProject) return;
    const path = await open({ filters: [{ name: "Reference", extensions: ["fb2", "txt", "pdf", "zip"] }] });
    if (typeof path !== "string") return;
    setBusyFor(activeId, t("busy.loadingReference"));
    addLog(t("log.loadingReference", { name: baseName(path) }));
    const info = await call<RefInfo>("load_reference", { projectId: activeId, path });
    setBusyFor(activeId, null);
    if (!info) return;
    book.setRef(info);
    setRefPath(activeId, path);
    addLog(t("log.reference", { n: info.max_covered ?? "?" }));
    if (info.imported > 0) addLog(t("log.refImported", { n: info.imported }));
    void book.refreshDetails();
    void job.refreshProgress();
    void book.loadChapters();
    const i = book.chapterIdxRef.current;
    if (i != null) void book.openChapter(i);
  }

  // Save the active project to a portable .bcproj archive.
  async function saveProject() {
    setMenu(null);
    if (!activeProject) return;
    const out = await save({
      defaultPath: `${activeProject.name}.bcproj`,
      filters: [{ name: "book-converter project", extensions: ["bcproj"] }],
    });
    if (typeof out !== "string") return;
    const path = out.toLowerCase().endsWith(".bcproj") ? out : `${out}.bcproj`;
    setBusyFor(activeId, t("busy.savingProject"));
    try {
      await invoke("export_project", { projectId: activeId, outPath: path });
      addLog(t("log.projectSaved", { path }));
    } catch (e) { logError(String(e)); }
    finally { setBusyFor(activeId, null); }
  }

  // Open a .bcproj archive as a new isolated project.
  async function openProjectArchive() {
    setMenu(null);
    const arch = await open({ filters: [{ name: "book-converter project", extensions: ["bcproj"] }] });
    if (typeof arch !== "string") return;
    const id = newId();
    setBusyFor(id, t("busy.openingProject"));
    try {
      const res = await invoke<{ name: string; source_path: string }>(
        "import_project", { projectId: id, archivePath: arch });
      // Leave busy set; activation (via setActive) will refresh and clear it.
      addProject({ id, path: res.source_path, name: res.name });
    } catch (e) {
      logError(String(e));
      setBusyFor(id, null);
    }
  }

  async function generateSummary() {
    setBusyFor(activeId, t("busy.generatingSummary"));
    try {
      await invoke<string>("generate_summary", { projectId: activeId });
      addLog(t("log.summaryGenerated"));
      void book.refreshDetails();
    } catch (e) {
      // Not finding the book is an expected outcome, not a critical error: log it.
      const msg = String(e);
      addLog(msg.includes("book_not_found") ? t("log.summaryNotFound") : t("log.error", { msg: errText(msg) }));
    } finally {
      setBusyFor(activeId, null);
    }
  }

  async function exportAs(fmt: "fb2" | "epub" | "pdf" | "txt") {
    setMenu(null);
    const outPath = await save({ defaultPath: `book.${fmt}`, filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }] });
    if (typeof outPath !== "string") return;
    const path = outPath.toLowerCase().endsWith(`.${fmt}`) ? outPath : `${outPath}.${fmt}`;
    setBusyFor(activeId, t("busy.exporting", { fmt: fmt.toUpperCase() }));
    const p = await call<string>("export_book", { projectId: activeId, outPath: path });
    setBusyFor(activeId, null);
    if (p) addLog(t("log.exported", { path: p }));
  }

  return {
    openBook, removeProject, openReference,
    saveProject, openProjectArchive,
    generateSummary, exportAs,
  };
}
