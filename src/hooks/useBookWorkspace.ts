import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { CallFn } from "../api";
import type { BookDetails, BookInfo, ChapterRow, ChapterView, RefInfo, Term } from "../types";

type Opts = {
  call: CallFn;
  activeId: string;
};

/** Book/ref/details/chapters/reader state and load helpers. */
export function useBookWorkspace({ call, activeId }: Opts) {
  const [book, setBook] = useState<BookInfo | null>(null);
  const [ref, setRef] = useState<RefInfo | null>(null);
  const [details, setDetails] = useState<BookDetails | null>(null);
  const [chapters, setChapters] = useState<ChapterRow[]>([]);
  const [chaptersLoading, setChaptersLoading] = useState(false);
  const [chapterIdx, setChapterIdx] = useState<number | null>(null);
  const chapterIdxRef = useRef<number | null>(null);
  const [chapter, setChapter] = useState<ChapterView | null>(null);
  const [chapterLoading, setChapterLoading] = useState(false);
  // Glossary terms that occur in THIS chapter, for the reader's highlighting.
  // The full glossary runs to tens of thousands of terms on a long book, and
  // scanning all of them against the chapter on every render is what made the
  // reader crawl; the backend already computes this set for the prompt.
  const [chapterTerms, setChapterTerms] = useState<Term[]>([]);
  const [panes, setPanes] = useState({ orig: true, transl: true });

  // Live refs: async loads must be checked against the project/list that is
  // current when they resolve, not the one captured when they started.
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  const chaptersRef = useRef<ChapterRow[]>(chapters);
  chaptersRef.current = chapters;

  function clearWorkspace() {
    setBook(null); setRef(null); setDetails(null);
    setChapters([]); setChapterIdx(null); setChapter(null);
    setChapterTerms([]);
    setChaptersLoading(false);
  }

  async function refreshDetails() {
    const d = await call<BookDetails>("get_book_details", { projectId: activeId });
    if (d) setDetails(d);
  }
  async function translateTitle() {
    const r = await call<string>("translate_title", { projectId: activeId });
    if (r) refreshDetails();
  }
  /**
   * Load the chapter list of `projectId` (the active project by default).
   * The explicit id lets project activation load chapters for the project it
   * just opened, without depending on when React re-renders.
   */
  async function loadChapters(projectId: string = activeId) {
    if (!projectId) return;
    // Only show the preloader on a first load; refreshes during a translation
    // run keep the existing list visible instead of flashing a skeleton.
    const first = chaptersRef.current.length === 0;
    if (first) setChaptersLoading(true);
    try {
      const cs = await call<ChapterRow[]>("list_chapters", { projectId });
      if (activeIdRef.current !== projectId) return; // switched project meanwhile
      if (cs) {
        setChapters(cs);
        if (chapterIdx == null && cs.length) {
          setChapterIdx((cs.find((c) => c.status === "done") || cs[0]).idx);
        }
      }
    } finally {
      if (first && activeIdRef.current === projectId) setChaptersLoading(false);
    }
  }
  /**
   * Reflect a saved manual edit in the loaded chapter and in the tree, instead
   * of refetching: the translation pane is edited in place, and a refetch would
   * swap the editor for a loading spinner on every autosave. The backend trims
   * what it stores, so the local copy is trimmed the same way.
   */
  function applyChapterEdit(idx: number, title: string, body: string) {
    const t = title.trim();
    const b = body.trim();
    setChapter((c) => (c && c.idx === idx
      ? { ...c, translated_title: t, translated: b, status: "done", origin: "manual" }
      : c));
    setChapters((cs) => cs.map((c) => (c.idx === idx
      ? { ...c, translated_title: t, status: "done", origin: "manual" }
      : c)));
  }

  async function openChapter(idx: number) {
    setChapterLoading(true);
    const projectId = activeId;
    const c = await call<ChapterView>("get_chapter", { projectId, index: idx });
    if (c) setChapter(c);
    setChapterLoading(false);
    void loadChapterTerms(idx, projectId);
  }

  /** Terms present in one chapter, for highlighting. Best-effort: a failure
   *  costs the underlines, not the chapter. */
  async function loadChapterTerms(idx: number, projectId: string = activeId) {
    const terms = await call<Term[]>("chapter_terms", { projectId, index: idx });
    // Ignore a load that resolved after the reader moved on.
    if (activeIdRef.current !== projectId || chapterIdxRef.current !== idx) return;
    setChapterTerms(terms ?? []);
  }

  useEffect(() => {
    chapterIdxRef.current = chapterIdx;
    if (chapterIdx != null) void openChapter(chapterIdx);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chapterIdx]);

  async function replaceCover() {
    const path = await open({ filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "gif", "webp"] }] });
    if (typeof path !== "string") return;
    await call("set_cover", { projectId: activeId, path });
    refreshDetails();
  }
  async function saveSummary(text: string) {
    await call("set_summary", { projectId: activeId, summary: text });
  }

  return {
    book, setBook,
    ref, setRef,
    details, setDetails,
    chapters, setChapters,
    chaptersLoading, setChaptersLoading,
    chapterIdx, setChapterIdx, chapterIdxRef,
    chapter, setChapter,
    chapterLoading,
    chapterTerms, loadChapterTerms,
    panes, setPanes,
    clearWorkspace,
    applyChapterEdit,
    refreshDetails,
    translateTitle,
    loadChapters,
    openChapter,
    replaceCover,
    saveSummary,
  };
}
