// Glossary highlight tokenizer.
//
// Splits chapter text into lines of tokens. A token carries `key` when it is a
// glossary match; `key` is the term identity (the source string) shared across
// both panes, so clicking one occurrence can highlight the mapped rendering in
// the other pane (source term <-> its target translation).

import { escapeRe } from "../types";

export type Token = { text: string; key?: string };
export type Line = Token[];

/** A literal to find (`match`) and the term identity it maps to (`key`). */
export type Match = { match: string; key: string };

const MAX_MATCHES = 400;

/** Tokenize `text` into lines, marking glossary matches with their term key. */
export function tokenizeLines(text: string, matches: Match[]): Line[] {
  const lines = text.split("\n");

  // Only terms actually present in this text; longest first so the most specific
  // match wins, and dedup identical match strings (cap keeps the regex bounded).
  const present = matches
    .filter((m) => m.match && text.includes(m.match))
    .sort((a, b) => b.match.length - a.match.length);
  const seen = new Set<string>();
  const uniq: Match[] = [];
  for (const m of present) {
    if (seen.has(m.match)) continue;
    seen.add(m.match);
    uniq.push(m);
    if (uniq.length >= MAX_MATCHES) break;
  }
  if (uniq.length === 0) return lines.map((l) => [{ text: l }]);

  const keyOf = new Map(uniq.map((m) => [m.match, m.key]));
  const re = new RegExp(`(${uniq.map((m) => escapeRe(m.match)).join("|")})`, "g");
  return lines.map((line) =>
    line
      .split(re)
      .filter((p) => p !== "")
      .map((part) => {
        const key = keyOf.get(part);
        return key ? { text: part, key } : { text: part };
      }),
  );
}
