// Literal find/replace helpers shared by the reader's find bar and the tokenizer.

import { escapeRe } from "../types";

export type FindOpts = { matchCase: boolean; wholeWord: boolean; regex?: boolean };

/**
 * Build a global regex for the query. Literal by default (word boundaries are
 * ASCII-only); with `regex` the query is used as a pattern, and an incomplete
 * one (typed halfway) yields null instead of throwing.
 */
export function findRegex(query: string, opts: FindOpts): RegExp | null {
  if (!query) return null;
  let pat = opts.regex ? query : escapeRe(query);
  if (opts.wholeWord) pat = `\\b${pat}\\b`;
  try {
    return new RegExp(pat, opts.matchCase ? "g" : "gi");
  } catch {
    return null;
  }
}

/** True when `regex` is on and the pattern does not compile (shown in the bar). */
export function isBadPattern(query: string, opts: FindOpts): boolean {
  return !!opts.regex && !!query && findRegex(query, opts) === null;
}

export function countMatches(text: string, query: string, opts: FindOpts): number {
  const re = findRegex(query, opts);
  if (!re) return 0;
  return (text.match(re) || []).length;
}

export function replaceAllText(text: string, query: string, replacement: string, opts: FindOpts): { text: string; count: number } {
  const re = findRegex(query, opts);
  if (!re) return { text, count: 0 };
  const count = (text.match(re) || []).length;
  // In regex mode the replacement may reference capture groups ($1), so it goes
  // to String.replace as a pattern; a literal search must never expand "$".
  const out = opts.regex ? text.replace(re, replacement) : text.replace(re, () => replacement);
  return { text: out, count };
}

/** Replace only the n-th (0-based) occurrence, leaving the rest untouched. */
export function replaceNth(text: string, query: string, replacement: string, opts: FindOpts, n: number): string {
  const re = findRegex(query, opts);
  if (!re) return text;
  let i = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (i === n) {
      // Expand capture groups against this match alone, so $1 works per hit.
      const value = opts.regex ? m[0].replace(findRegex(query, opts)!, replacement) : replacement;
      return text.slice(0, m.index) + value + text.slice(m.index + m[0].length);
    }
    i++;
    if (m[0].length === 0) re.lastIndex++;
  }
  return text;
}

/**
 * Text currently selected on the page. Reads the caret selection of a focused
 * field directly: `window.getSelection()` returns nothing for the contents of a
 * textarea, which is where the reader's editor lives.
 */
export function selectedText(): string {
  const el = document.activeElement;
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
    return el.value.slice(el.selectionStart ?? 0, el.selectionEnd ?? 0);
  }
  return window.getSelection()?.toString() ?? "";
}
