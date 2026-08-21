import { TERM_KINDS, type Progress, type Term } from "../types";
import { VirtualList } from "./common/VirtualList";
import { Panel } from "./Panel";

/** Row height in px. Must match `.glossary-row` in styles.css: the windowing
 *  list positions rows arithmetically, so a mismatch shifts the whole list. */
const ROW_HEIGHT = 34;

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  collapsed: Record<string, boolean>;
  onToggle: (id: string) => void;
  /** The window of terms loaded so far (not the whole glossary). */
  terms: Term[];
  /** How many terms match the current filter, loaded or not. */
  total: number;
  loading: boolean;
  glossaryQuery: string;
  setGlossaryQuery: (q: string) => void;
  kindFilter: string;
  setKindFilter: (k: string) => void;
  onLoadMore: () => void;
  newTerm: { source: string; target: string; kind: string };
  setNewTerm: (v: { source: string; target: string; kind: string }) => void;
  pending: Record<string, { old: string; new: string; kind: string }>;
  pendingCount: number;
  progress: Progress | null;
  onUpdateTranslation: () => void;
  onRefreshGlossary: () => void;
  onAddTerm: () => void;
  onRenameTerm: (term: Term, source: string) => void;
  onEditTarget: (term: Term, value: string) => void;
  onEditKind: (term: Term, kind: string) => void;
  onDeleteTerm: (term: Term) => void;
};

/**
 * The glossary table.
 *
 * Rows are windowed and paged: a book's glossary reaches tens of thousands of
 * terms, and rendering that many live rows (each with two inputs and a select)
 * froze the app. Only the rows in view exist in the DOM, and the backend is
 * asked for the next page as the list is scrolled.
 */
export function GlossaryView({
  t, collapsed, onToggle, terms, total, loading,
  glossaryQuery, setGlossaryQuery, kindFilter, setKindFilter, onLoadMore,
  newTerm, setNewTerm, pending, pendingCount, progress,
  onUpdateTranslation, onRefreshGlossary, onAddTerm,
  onRenameTerm, onEditTarget, onEditKind, onDeleteTerm,
}: Props) {
  return (
    <Panel id="glossary" title={t("glossary.title", { n: total })} collapsed={collapsed} onToggle={onToggle} extra={
      <>
        <button className="primary" disabled={!pendingCount || !(progress && progress.done > 0)}
          title={t("glossary.updateTranslationTip")} onClick={onUpdateTranslation}>
          {t("glossary.updateTranslation")}{pendingCount ? ` (${pendingCount})` : ""}
        </button>
        <input placeholder={t("glossary.filter")} value={glossaryQuery} onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 140 }} />
        <button onClick={onRefreshGlossary}>↻</button>
      </>
    }>
      <div className="kind-filter">
        <button className={`chip ${kindFilter === "all" ? "on" : ""}`} onClick={() => setKindFilter("all")}>{t("glossary.kindAll")}</button>
        {TERM_KINDS.map((k) => (
          <button key={k} className={`chip kind-chip kind-${k} ${kindFilter === k ? "on" : ""}`} onClick={() => setKindFilter(k)}>
            <span className={`kind-dot kind-${k}`} />{t(`kind.${k}`)}
          </button>
        ))}
      </div>

      <div className="glossary-grid">
        <div className="glossary-row glossary-head">
          <span>{t("glossary.colSource")}</span>
          <span>{t("glossary.colTranslation")}</span>
          <span>{t("glossary.colKind")}</span>
          <span>{t("glossary.colCount")}</span>
          <span>{t("glossary.colActions")}</span>
        </div>

        <div className="glossary-row glossary-add">
          <input placeholder={t("glossary.addSource")} value={newTerm.source}
            onChange={(e) => setNewTerm({ ...newTerm, source: e.target.value })} />
          <input placeholder={t("glossary.addTranslation")} value={newTerm.target}
            onChange={(e) => setNewTerm({ ...newTerm, target: e.target.value })}
            onKeyDown={(e) => e.key === "Enter" && onAddTerm()} />
          <select value={newTerm.kind} onChange={(e) => setNewTerm({ ...newTerm, kind: e.target.value })}>
            {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
          </select>
          <span />
          <button onClick={onAddTerm} disabled={!newTerm.source.trim() || !newTerm.target.trim()}>{t("glossary.add")}</button>
        </div>

        {terms.length === 0 ? (
          <div className="empty">{loading ? t("glossary.loading") : t("glossary.empty")}</div>
        ) : (
          <VirtualList
            className="glossary-rows"
            items={terms}
            rowHeight={ROW_HEIGHT}
            onReachEnd={onLoadMore}
            renderRow={(term) => (
              <div key={term.source} className={`glossary-row ${pending[term.source] ? "dirty" : ""}`}>
                <input defaultValue={term.source} key={`s${term.source}`}
                  onBlur={(ev) => { const v = ev.target.value.trim(); if (v && v !== term.source) onRenameTerm(term, v); }} />
                <input defaultValue={term.target} key={`t${term.source}:${term.target}`}
                  onBlur={(ev) => onEditTarget(term, ev.target.value)} />
                <div className="kind-cell">
                  <span className={`kind-dot kind-${term.kind}`} />
                  <select value={term.kind} onChange={(ev) => onEditKind(term, ev.target.value)}>
                    {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
                  </select>
                </div>
                <span className="glossary-count">{term.frequency}{term.pinned ? " 📌" : ""}</span>
                <button className="ghost del" title={t("glossary.delete")} onClick={() => onDeleteTerm(term)}>✕</button>
              </div>
            )}
          />
        )}

        {terms.length > 0 && (
          <div className="glossary-foot muted">
            {t("glossary.shownOf", { shown: terms.length, total })}
            {loading ? ` · ${t("glossary.loading")}` : ""}
          </div>
        )}
      </div>
    </Panel>
  );
}
