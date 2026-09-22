// Shared chapter helpers used by the explorer tree, tab bar, and command palette.

import type { ChapterRow } from "../types";

/** What a chapter's marker in the tree means. */
export type MarkerId =
  | "failed" | "issues" | "in_progress" | "image" | "reference" | "manual" | "done" | "pending";

export const MARKER_GLYPHS: Record<MarkerId, string> = {
  failed: "✕",
  issues: "⚠",
  in_progress: "◌",
  image: "▣",
  reference: "◆",
  manual: "✎",
  done: "✓",
  pending: "·",
};

/** Legend order: problems first, then provenance, then plain progress. */
export const MARKER_ORDER: MarkerId[] = [
  "failed", "issues", "in_progress", "image", "reference", "manual", "done", "pending",
];

/**
 * Which marker a chapter gets. Problems outrank provenance: a failed chapter, or
 * one whose translation kept foreign words, is what has to stand out in a
 * 1350-row tree. A page of pictures is marked as such, because its lack of a
 * translation is the finished state, not a missing one.
 */
export function chapterMarker(c: ChapterRow): MarkerId {
  if (c.status === "failed") return "failed";
  if (c.lang_issues) return "issues";
  if (c.status === "in_progress") return "in_progress";
  if (c.kind === "image" || c.kind === "empty") return "image";
  if (c.origin === "reference") return "reference";
  if (c.origin === "manual") return "manual";
  if (c.status === "done") return "done";
  return "pending";
}

/** Status/origin glyph shown before a chapter label. */
export function chapterGlyph(c: ChapterRow): string {
  return MARKER_GLYPHS[chapterMarker(c)];
}

/** Why a chapter is flagged in the tree, for its tooltip. */
export function chapterIssue(c: ChapterRow): string | null {
  if (c.status === "failed") return "failed";
  if (c.lang_issues) return c.lang_issues;
  return null;
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
