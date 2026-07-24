import type { FindMode, FindScope } from "../../hooks/useFindReplace";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  mode: FindMode;
  setMode: (m: FindMode) => void;
  query: string;
  setQuery: (v: string) => void;
  replacement: string;
  setReplacement: (v: string) => void;
  matchCase: boolean;
  setMatchCase: (v: boolean) => void;
  wholeWord: boolean;
  setWholeWord: (v: boolean) => void;
  scope: FindScope;
  setScope: (v: FindScope) => void;
  count: number;
  current: number;
  busy: boolean;
  onPrev: () => void;
  onNext: () => void;
  onReplaceOne: () => void;
  onReplaceAll: () => void;
  onClose: () => void;
};

/** The reader's find/replace bar: search the current chapter, replace here or book-wide. */
export function FindReplaceBar({
  t, mode, setMode, query, setQuery, replacement, setReplacement,
  matchCase, setMatchCase, wholeWord, setWholeWord, scope, setScope,
  count, current, busy, onPrev, onNext, onReplaceOne, onReplaceAll, onClose,
}: Props) {
  const label = count > 0 ? t("find.countOf", { current: current + 1, total: count }) : t("find.noMatches");

  return (
    <div className="findbar">
      <button className="findbar-toggle" title={t("find.toggleReplace")} onClick={() => setMode(mode === "find" ? "replace" : "find")}>
        {mode === "replace" ? "▾" : "▸"}
      </button>

      <div className="findbar-rows">
        <div className="findbar-row">
          <input
            className="findbar-input"
            autoFocus
            placeholder={t("find.findPlaceholder")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); e.shiftKey ? onPrev() : onNext(); }
              else if (e.key === "Escape") { e.preventDefault(); onClose(); }
            }}
          />
          <span className="findbar-count">{label}</span>
          <button className="chip" title={t("find.matchCase")} data-on={matchCase} onClick={() => setMatchCase(!matchCase)}>Aa</button>
          <button className="chip" title={t("find.wholeWord")} data-on={wholeWord} onClick={() => setWholeWord(!wholeWord)}>W</button>
          <button className="icon" disabled={count === 0} title={t("find.prev")} onClick={onPrev}>‹</button>
          <button className="icon" disabled={count === 0} title={t("find.next")} onClick={onNext}>›</button>
          <button className="icon" title={t("find.close")} onClick={onClose}>×</button>
        </div>

        {mode === "replace" && (
          <div className="findbar-row">
            <input
              className="findbar-input"
              placeholder={t("find.replacePlaceholder")}
              value={replacement}
              onChange={(e) => setReplacement(e.target.value)}
            />
            <select value={scope} onChange={(e) => setScope(e.target.value as FindScope)} title={t("find.scopeTip")}>
              <option value="chapter">{t("find.scopeChapter")}</option>
              <option value="book">{t("find.scopeBook")}</option>
            </select>
            <button className="ghost" disabled={busy || scope === "book" || count === 0} onClick={onReplaceOne}>{t("find.replaceOne")}</button>
            <button disabled={busy || !query} onClick={onReplaceAll}>{busy ? t("find.replacing") : t("find.replaceAll")}</button>
          </div>
        )}
      </div>
    </div>
  );
}
