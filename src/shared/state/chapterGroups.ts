import type { ChapterSummary } from "../contracts/generated";
export type ChapterRow =
  | { kind: "volume"; key: string; label: string; source: string; count: number; collapsed: boolean }
  | { kind: "chapter"; chapter: ChapterSummary };

/** Group consecutive chapters without changing reading order or original IDs. */
export function chapterRows(chapters: ChapterSummary[], collapsed: ReadonlySet<string>, projectId: string, searching = false): ChapterRow[] {
  const rows: ChapterRow[] = [];
  for (let start = 0; start < chapters.length;) {
    const volume = chapters[start].volume;
    if (!volume) {
      rows.push({ kind: "chapter", chapter: chapters[start++] });
      continue;
    }
    let end = start + 1;
    while (end < chapters.length && chapters[end].volume === volume) end++;
    const key = `${projectId}/${volume}`;
    const closed = !searching && collapsed.has(key);
    rows.push({ kind: "volume", key, label: chapters[start].translatedVolume || volume, source: volume, count: end - start, collapsed: closed });
    if (!closed) for (const chapter of chapters.slice(start, end)) rows.push({ kind: "chapter", chapter });
    start = end;
  }
  return rows;
}
