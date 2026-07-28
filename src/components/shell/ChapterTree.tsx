import { useMemo, useState } from "react";
import { chapterGlyph, chapterIssue, chapterLabel, chapterMatches } from "../../lib/chapters";
import type { ChapterRow } from "../../types";
import { SkeletonList } from "../common/Skeleton";
import { VirtualList } from "../common/VirtualList";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  chapters: ChapterRow[];
  /** Chapters are being fetched: show placeholders, not an empty state. */
  loading?: boolean;
  activeIdx: number | null;
  onOpen: (idx: number) => void;
};

/** Virtualized, filterable chapter list for the explorer (handles ~1500 rows). */
export function ChapterTree({ t, chapters, loading, activeIdx, onOpen }: Props) {
  const [q, setQ] = useState("");
  const filtered = useMemo(
    () => (q.trim() ? chapters.filter((c) => chapterMatches(c, q)) : chapters),
    [chapters, q],
  );

  return (
    <div className="chtree">
      <div className="chtree-search">
        <input
          placeholder={t("explorer.filterChapters")} value={q}
          disabled={loading} onChange={(e) => setQ(e.target.value)}
        />
        <span className="muted chtree-count">
          {loading ? <span className="spinner tiny" /> : filtered.length}
        </span>
      </div>
      {loading ? (
        <div className="chtree-list">
          <div className="chtree-loading muted">{t("explorer.loadingChapters")}</div>
          <SkeletonList rows={10} rowHeight={26} />
        </div>
      ) : filtered.length === 0 ? (
        <div className="empty">{t("explorer.noChapters")}</div>
      ) : (
        <VirtualList
          className="chtree-list"
          items={filtered}
          rowHeight={26}
          renderRow={(c) => (
            <div
              key={c.idx}
              className={`chtree-row ${c.idx === activeIdx ? "active" : ""} ${chapterIssue(c) ? "flagged" : ""}`}
              onClick={() => onOpen(c.idx)}
              title={chapterIssue(c) ? `${chapterLabel(c)} — ${chapterIssue(c)}` : chapterLabel(c)}
            >
              <span className="chtree-glyph">{chapterGlyph(c)}</span>
              {c.number != null && <span className="chtree-num">#{c.number}</span>}
              <span className="chtree-title">{chapterLabel(c)}</span>
            </div>
          )}
        />
      )}
    </div>
  );
}
