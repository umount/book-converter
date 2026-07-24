import { useEffect, useState } from "react";
import type { ViewId } from "../types";

type Opts = {
  view: ViewId;
  setView: (v: ViewId) => void;
  chapterIdx: number | null;
  setChapterIdx: (idx: number) => void;
  activeId: string;
};

/**
 * Editor tabs on top of the existing `view` + `chapterIdx` state.
 *
 * `view` stays the "active document kind"; the reader's active chapter is
 * `chapterIdx`. This hook only tracks which chapters have an open tab: any path
 * that shows a chapter in the reader gets a tab, and closing the active tab moves
 * to a neighbor (or back to Overview). Tabs reset when the project changes.
 */
export function useTabs({ view, setView, chapterIdx, setChapterIdx, activeId }: Opts) {
  const [openChapters, setOpenChapters] = useState<number[]>([]);

  useEffect(() => {
    setOpenChapters([]);
  }, [activeId]);

  // Ensure the chapter shown in the reader always has a tab.
  useEffect(() => {
    if (view === "reader" && chapterIdx != null) {
      setOpenChapters((o) => (o.includes(chapterIdx) ? o : [...o, chapterIdx]));
    }
  }, [view, chapterIdx]);

  function openChapter(idx: number) {
    setView("reader");
    setChapterIdx(idx);
  }

  function closeChapter(idx: number) {
    const pos = openChapters.indexOf(idx);
    const next = openChapters.filter((x) => x !== idx);
    setOpenChapters(next);
    if (view === "reader" && chapterIdx === idx) {
      if (next.length === 0) setView("overview");
      else setChapterIdx(next[Math.min(pos, next.length - 1)]);
    }
  }

  return { openChapters, openChapter, closeChapter };
}
