import { createElement, type ReactNode } from "react";

// --- DTOs (mirror src-tauri/src/commands.rs) ---
export type BookInfo = {
  title: string; author: string; total_chapters: number;
  format: string; encoding: string; needs_delimiter: boolean; missing: number; duplicates: number;
};
export type RefInfo = { title: string; chapters: number; max_covered: number | null; imported: number };
export type Progress = {
  project: string;
  done: number;
  total: number;
  failed: number;
  pending: number;
  running: boolean;
  job_done?: number;
  job_total?: number;
  current_idx?: number | null;
  /** Book chapter number from the title (`第N章`), when known. */
  current_number?: number | null;
  current_title?: string | null;
  /** First still-pending chapter's book number. */
  next_number?: number | null;
  max_number?: number | null;
  phase?: string;
  last_ms?: number | null;
  eta_secs?: number | null;
};
export type Term = { source: string; target: string; kind: string; frequency: number; pinned: boolean };
export const TERM_KINDS = ["person", "location", "organization", "term"] as const;
// Language names understood by the model in prompts ("translate from X to Y").
export const TRANSLATION_LANGS = [
  "English", "Russian", "Chinese", "Japanese", "Korean",
  "German", "French", "Spanish", "Italian", "Portuguese",
] as const;
export type BookDetails = {
  title: string; author: string; title_translated: string | null; author_translated: string | null;
  summary: string | null; cover: string | null;
};
export type ChapterRow = { idx: number; number: number | null; title: string; status: string; origin: string | null };
export type ChapterView = {
  idx: number; number: number | null; source_title: string; source: string;
  translated_title: string | null; translated: string | null; status: string; origin: string | null;
  /** Per-chapter instruction injected into the translation prompt. */
  user_prompt: string | null;
  /** Rolling story synopsis used when translating this chapter. */
  rolling_summary: string | null;
  /** Previous chapter ending used for continuity. */
  prev_tail: string | null;
};

export type Project = { id: string; path: string; name: string; refPath?: string };
export type ViewId = "overview" | "reader" | "glossary";

export const LS_PROJECTS = "bc.projects.v2";
export const LS_ACTIVE = "bc.active.v2";

export const baseName = (p: string) => p.split(/[\\/]/).pop() || p;
export const newId = () =>
  (crypto.randomUUID ? crypto.randomUUID() : `p-${Date.now()}-${Math.random().toString(36).slice(2)}`);
export const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

// Wrap glossary terms present in `text` with <mark>.
export function highlight(text: string, terms: string[]): ReactNode[] {
  const present = Array.from(new Set(terms.filter((t) => t && text.includes(t))))
    .sort((a, b) => b.length - a.length)
    .slice(0, 400);
  if (present.length === 0) return [text];
  const re = new RegExp(`(${present.map(escapeRe).join("|")})`, "g");
  return text.split(re).map((part, i) =>
    (i % 2 === 1 ? createElement("mark", { key: i }, part) : part));
}
