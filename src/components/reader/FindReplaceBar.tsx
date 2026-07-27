import { useEffect, useRef } from "react";
import type { FindApi } from "../../hooks/useFindReplace";
import type { FindScope } from "../../hooks/useFindReplace";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  find: FindApi;
  /** Matches in the current chapter (0 when the pattern is invalid). */
  count: number;
  /** Regex mode with a pattern that does not compile yet. */
  badPattern: boolean;
  busy: boolean;
  onPrev: () => void;
  onNext: () => void;
  onReplaceOne: () => void;
  onReplaceAll: () => void;
};

/** The reader's find/replace bar: search the current chapter, replace here or book-wide. */
export function FindReplaceBar({
  t, find, count, badPattern, busy, onPrev, onNext, onReplaceOne, onReplaceAll,
}: Props) {
  const {
    mode, setMode, query, setQuery, replacement, setReplacement,
    matchCase, setMatchCase, wholeWord, setWholeWord, regex, setRegex,
    scope, setScope, current, focusToken, close,
  } = find;
  const inputRef = useRef<HTMLInputElement>(null);

  // Re-focus (and select) whenever the bar is (re)opened, so ⌘F while it is
  // already open grabs the field back instead of doing nothing.
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusToken]);

  const label = badPattern
    ? t("find.badPattern")
    : count > 0
      ? t("find.countOf", { current: current + 1, total: count })
      : t("find.noMatches");

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") { e.preventDefault(); if (e.shiftKey) onPrev(); else onNext(); }
    else if (e.key === "Escape") { e.preventDefault(); close(); }
  }

  return (
    <div className="findbar">
      <button className="findbar-toggle" title={t("find.toggleReplace")} onClick={() => setMode(mode === "find" ? "replace" : "find")}>
        {mode === "replace" ? "▾" : "▸"}
      </button>

      <div className="findbar-rows">
        <div className="findbar-row">
          <input
            ref={inputRef}
            className={`findbar-input ${badPattern ? "invalid" : ""}`}
            placeholder={t("find.findPlaceholder")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <span className={`findbar-count ${badPattern ? "invalid" : ""}`}>{label}</span>
          <button className="chip" title={t("find.matchCase")} data-on={matchCase} onClick={() => setMatchCase(!matchCase)}>Aa</button>
          <button className="chip" title={t("find.wholeWord")} data-on={wholeWord} onClick={() => setWholeWord(!wholeWord)}>W</button>
          <button className="chip" title={t("find.regex")} data-on={regex} onClick={() => setRegex(!regex)}>.*</button>
          <button className="icon" disabled={count === 0} title={t("find.prev")} onClick={onPrev}>‹</button>
          <button className="icon" disabled={count === 0} title={t("find.next")} onClick={onNext}>›</button>
          <button className="icon" title={t("find.close")} onClick={close}>×</button>
        </div>

        {mode === "replace" && (
          <div className="findbar-row">
            <input
              className="findbar-input"
              placeholder={regex ? t("find.replaceGroupsPlaceholder") : t("find.replacePlaceholder")}
              value={replacement}
              onChange={(e) => setReplacement(e.target.value)}
              onKeyDown={onKeyDown}
            />
            <select value={scope} onChange={(e) => setScope(e.target.value as FindScope)} title={t("find.scopeTip")}>
              <option value="chapter">{t("find.scopeChapter")}</option>
              <option value="book">{t("find.scopeBook")}</option>
            </select>
            <button className="ghost" disabled={busy || scope === "book" || count === 0} onClick={onReplaceOne}>{t("find.replaceOne")}</button>
            <button disabled={busy || !query || badPattern} onClick={onReplaceAll}>{busy ? t("find.replacing") : t("find.replaceAll")}</button>
          </div>
        )}
      </div>
    </div>
  );
}
