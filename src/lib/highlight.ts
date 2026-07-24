// Glossary + search highlight tokenizer.
//
// Splits chapter text into lines of tokens. A token may carry:
//   - `key`: a glossary match's term identity (the source string), shared across
//     both panes so clicking one occurrence highlights the mapped rendering in
//     the other pane (source term <-> its target translation);
//   - `search`: the global (document-order) index of a find-bar match, so the
//     current match can be marked and scrolled into view.
// Search matches take priority and are numbered in the same order the find bar
// counts them, so occurrence indices line up.

import { escapeRe } from "../types";
import { findRegex, type FindOpts } from "./find";

export type Token = { text: string; key?: string; search?: number };
export type Line = Token[];

/** A literal to find (`match`) and the term identity it maps to (`key`). */
export type Match = { match: string; key: string };
export type SearchSpec = { query: string; opts: FindOpts };

const MAX_MATCHES = 400;

function glossaryRegex(text: string, matches: Match[]): { re: RegExp | null; keyOf: Map<string, string> } {
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
  if (uniq.length === 0) return { re: null, keyOf: new Map() };
  const keyOf = new Map(uniq.map((m) => [m.match, m.key]));
  return { re: new RegExp(`(${uniq.map((m) => escapeRe(m.match)).join("|")})`, "g"), keyOf };
}

function pushGlossary(out: Token[], text: string, re: RegExp | null, keyOf: Map<string, string>) {
  if (!text) return;
  if (!re) { out.push({ text }); return; }
  for (const part of text.split(re)) {
    if (part === "") continue;
    const key = keyOf.get(part);
    out.push(key ? { text: part, key } : { text: part });
  }
}

/** Tokenize `text` into lines, marking glossary terms and (optionally) search hits. */
export function tokenizeLines(text: string, matches: Match[], search?: SearchSpec | null): Line[] {
  const { re: gre, keyOf } = glossaryRegex(text, matches);
  const searchRe = search && search.query ? findRegex(search.query, search.opts) : null;
  let counter = 0;
  return text.split("\n").map((line) => {
    const tokens: Token[] = [];
    if (searchRe) {
      searchRe.lastIndex = 0;
      let last = 0;
      let m: RegExpExecArray | null;
      while ((m = searchRe.exec(line)) !== null) {
        if (m.index > last) pushGlossary(tokens, line.slice(last, m.index), gre, keyOf);
        tokens.push({ text: m[0], search: counter++ });
        last = m.index + m[0].length;
        if (m[0].length === 0) searchRe.lastIndex++;
      }
      if (last < line.length) pushGlossary(tokens, line.slice(last), gre, keyOf);
    } else {
      pushGlossary(tokens, line, gre, keyOf);
    }
    return tokens;
  });
}
