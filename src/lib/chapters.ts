// Shared chapter helpers used by the explorer tree, tab bar, and command palette.

import type { ChapterRow } from "../types";

/** Status/origin glyph shown before a chapter label. */
export function chapterGlyph(c: ChapterRow): string {
  if (c.origin === "reference") return "◆";
  if (c.origin === "manual") return "✎";
  if (c.status === "done") return "✓";
  if (c.status === "failed") return "✕";
  return "·";
}

/** Preferred display title: translated title if present, else the source title. */
export function chapterLabel(c: ChapterRow): string {
  return (c.translated_title?.trim() || c.title || "").trim();
}

/** Case-insensitive match on chapter number, source title, or translated title. */
export function chapterMatches(c: ChapterRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (c.number != null && String(c.number).includes(q)) return true;
  return c.title.toLowerCase().includes(q) || !!c.translated_title?.toLowerCase().includes(q);
}
