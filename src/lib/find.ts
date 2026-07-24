// Literal find/replace helpers shared by the reader's find bar and the tokenizer.

import { escapeRe } from "../types";

export type FindOpts = { matchCase: boolean; wholeWord: boolean };

/** Build a global regex for a literal query (word boundaries are ASCII-only). */
export function findRegex(query: string, opts: FindOpts): RegExp | null {
  if (!query) return null;
  let pat = escapeRe(query);
  if (opts.wholeWord) pat = `\\b${pat}\\b`;
  try {
    return new RegExp(pat, opts.matchCase ? "g" : "gi");
  } catch {
    return null;
  }
}

export function countMatches(text: string, query: string, opts: FindOpts): number {
  const re = findRegex(query, opts);
  if (!re) return 0;
  return (text.match(re) || []).length;
}

export function replaceAllText(text: string, query: string, replacement: string, opts: FindOpts): { text: string; count: number } {
  const re = findRegex(query, opts);
  if (!re) return { text, count: 0 };
  let count = 0;
  const out = text.replace(re, () => { count++; return replacement; });
  return { text: out, count };
}

/** Replace only the n-th (0-based) occurrence, leaving the rest untouched. */
export function replaceNth(text: string, query: string, replacement: string, opts: FindOpts, n: number): string {
  const re = findRegex(query, opts);
  if (!re) return text;
  let i = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (i === n) return text.slice(0, m.index) + replacement + text.slice(m.index + m[0].length);
    i++;
    if (m[0].length === 0) re.lastIndex++;
  }
  return text;
}
