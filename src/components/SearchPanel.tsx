import { useEffect, useRef, useState } from "react";
import type { CallFn } from "../api";
import { isBadPattern, type FindOpts } from "../lib/find";
import { tokenizeLines } from "../lib/highlight";
import type { SearchChapter } from "../types";
import { VirtualList } from "./common/VirtualList";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  call: CallFn;
  activeId: string;
  /** Bumped by ⌘⇧F so the field takes focus even when the panel is already open. */
  focusToken: number;
  /** Open a chapter and highlight `query` there. */
  onOpenHit: (idx: number, query: string, opts: FindOpts) => void;
};

/** How long to wait after the last keystroke before searching the whole book. */
const DEBOUNCE_MS = 300;

type Row =
  | { kind: "chapter"; chapter: SearchChapter }
  | { kind: "hit"; chapter: SearchChapter; line: number; preview: string };

/** Book-wide search results, grouped per chapter (the IDE's search view). */
export function SearchPanel({ t, call, activeId, focusToken, onOpenHit }: Props) {
  const [query, setQuery] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [regex, setRegex] = useState(false);
  const [inSource, setInSource] = useState(false);
  const [results, setResults] = useState<SearchChapter[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [collapsed, setCollapsed] = useState<Record<number, boolean>>({});
  const inputRef = useRef<HTMLInputElement>(null);

  const opts: FindOpts = { matchCase, wholeWord, regex };
  const badPattern = isBadPattern(query, opts);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusToken]);

  // Re-run on every input change (debounced) and whenever an option flips.
  useEffect(() => {
    if (!query.trim() || badPattern || !activeId) {
      setResults(null);
      return;
    }
    const id = setTimeout(async () => {
      setBusy(true);
      const r = await call<SearchChapter[]>("search_book", {
        projectId: activeId, query, matchCase, wholeWord, regex, inSource,
      });
      setResults(r ?? []);
      setBusy(false);
    }, DEBOUNCE_MS);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, matchCase, wholeWord, regex, inSource, activeId]);

  const total = results?.reduce((n, c) => n + c.count, 0) ?? 0;
  const rows: Row[] = (results ?? []).flatMap((chapter) => [
    { kind: "chapter" as const, chapter },
    ...(collapsed[chapter.idx]
      ? []
      : chapter.hits.map((h) => ({ kind: "hit" as const, chapter, line: h.line, preview: h.preview }))),
  ]);

  return (
    <div className="searchpanel">
      <div className="sidebar-head"><span>{t("search.title")}</span></div>

      <div className="searchpanel-form">
        <input
          ref={inputRef}
          className={`searchpanel-input ${badPattern ? "invalid" : ""}`}
          placeholder={t("search.placeholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="searchpanel-opts">
          <button className="chip" title={t("find.matchCase")} data-on={matchCase} onClick={() => setMatchCase(!matchCase)}>Aa</button>
          <button className="chip" title={t("find.wholeWord")} data-on={wholeWord} onClick={() => setWholeWord(!wholeWord)}>W</button>
          <button className="chip" title={t("find.regex")} data-on={regex} onClick={() => setRegex(!regex)}>.*</button>
          <div className="menu-spacer" />
          <button
            className="chip" data-on={inSource} title={t("search.inSourceTip")}
            onClick={() => setInSource(!inSource)}
          >
            {inSource ? t("search.inSource") : t("search.inTranslation")}
          </button>
        </div>
        <div className="searchpanel-summary muted">
          {badPattern
            ? t("find.badPattern")
            : busy
              ? t("search.searching")
              : results
                ? t("search.summary", { hits: total, chapters: results.length })
                : t("search.hint")}
        </div>
      </div>

      {rows.length > 0 && (
        <VirtualList
          className="searchpanel-results"
          items={rows}
          rowHeight={24}
          renderRow={(row) =>
            row.kind === "chapter" ? (
              <div
                className="search-chapter"
                key={`c${row.chapter.idx}`}
                onClick={() => setCollapsed((c) => ({ ...c, [row.chapter.idx]: !c[row.chapter.idx] }))}
                title={row.chapter.title}
              >
                <span className={`chevron ${collapsed[row.chapter.idx] ? "closed" : ""}`}>▾</span>
                {row.chapter.number != null && <span className="chtree-num">#{row.chapter.number}</span>}
                <span className="search-chapter-title">{row.chapter.title}</span>
                <span className="search-count">{row.chapter.count}</span>
              </div>
            ) : (
              <div
                className="search-hit"
                key={`h${row.chapter.idx}-${row.line}`}
                onClick={() => onOpenHit(row.chapter.idx, query, opts)}
                title={row.preview}
              >
                <span className="search-line">{row.line}</span>
                <span className="search-preview">{highlight(row.preview, query, opts)}</span>
              </div>
            )
          }
        />
      )}
    </div>
  );
}

/** Mark the query inside a preview line, reusing the reader's tokenizer. */
function highlight(preview: string, query: string, opts: FindOpts) {
  const [tokens] = tokenizeLines(preview, [], { query, opts });
  return tokens.map((tk, i) =>
    tk.search != null ? <mark key={i}>{tk.text}</mark> : <span key={i}>{tk.text}</span>,
  );
}
