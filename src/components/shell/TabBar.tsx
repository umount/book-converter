import { useMemo } from "react";
import { chapterLabel } from "../../lib/chapters";
import type { ChapterRow, ViewId } from "../../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  view: ViewId;
  chapters: ChapterRow[];
  openChapters: number[];
  chapterIdx: number | null;
  glossaryCount: number;
  onSelectView: (v: ViewId) => void;
  onSelectChapter: (idx: number) => void;
  onCloseChapter: (idx: number) => void;
};

/** Editor tab strip: fixed Overview / Glossary tabs plus one tab per open chapter. */
export function TabBar({
  t, view, chapters, openChapters, chapterIdx, glossaryCount,
  onSelectView, onSelectChapter, onCloseChapter,
}: Props) {
  const byIdx = useMemo(() => new Map(chapters.map((c) => [c.idx, c])), [chapters]);

  return (
    <div className="tabbar" role="tablist">
      <div className={`tab ${view === "overview" ? "active" : ""}`} onClick={() => onSelectView("overview")}>
        {t("nav.overview")}
      </div>
      <div className={`tab ${view === "glossary" ? "active" : ""}`} onClick={() => onSelectView("glossary")}>
        {t("nav.glossary")}
        <span className="tab-badge">{glossaryCount}</span>
      </div>
      {openChapters.map((idx) => {
        const c = byIdx.get(idx);
        const label = (c && chapterLabel(c)) || `#${c?.number ?? idx}`;
        const active = view === "reader" && chapterIdx === idx;
        return (
          <div key={idx} className={`tab ${active ? "active" : ""}`} onClick={() => onSelectChapter(idx)} title={label}>
            {c?.number != null && <span className="tab-num">#{c.number}</span>}
            <span className="tab-label">{label.slice(0, 30)}</span>
            <button className="tab-close" title={t("tabs.close")} onClick={(e) => { e.stopPropagation(); onCloseChapter(idx); }}>×</button>
          </div>
        );
      })}
    </div>
  );
}
