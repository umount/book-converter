import { useState } from "react";
import { TERM_KINDS, type Progress, type Term } from "../types";
import { VirtualList } from "./common/VirtualList";
import { TermDialog } from "./glossary/TermDialog";
import { Panel } from "./Panel";

/** Row height in px. Must match `.glossary-row` in styles.css: the windowing
 *  list positions rows arithmetically, so a mismatch shifts the whole list. */
const ROW_HEIGHT = 30;

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
  pending: Record<string, { old: string; new: string; kind: string }>;
  pendingCount: number;
  progress: Progress | null;
  onUpdateTranslation: () => void;
  onRefreshGlossary: () => void;
  /** Create or update one term. `original` is null when adding. */
  onSaveTerm: (next: Term, original: Term | null) => Promise<void>;
  onDeleteTerm: (term: Term) => void;
};

/**
 * The glossary table.
 *
 * Read-only rows. Editing happens in a dialog and writes only on Save: the
 * table used to be editable in place and saved on blur, so a stray click could
 * change a term, and changing a rendering queues a model-driven rewrite of
 * every affected paragraph in the book. That is not something a misclick should
 * start.
 *
 * Rows are windowed and paged: a book's glossary reaches tens of thousands of
 * terms, and rendering that many live rows froze the app. Plain text rows are
 * also far cheaper than five form controls each.
 */
export function GlossaryView({
  t, collapsed, onToggle, terms, total, loading,
  glossaryQuery, setGlossaryQuery, kindFilter, setKindFilter, onLoadMore,
  pending, pendingCount, progress,
  onUpdateTranslation, onRefreshGlossary, onSaveTerm, onDeleteTerm,
}: Props) {
  // `null` = closed; `{ term: null }` = adding; `{ term }` = editing.
  const [dialog, setDialog] = useState<{ term: Term | null } | null>(null);
  const hasTranslation = !!progress && progress.done > 0;

  return (
    <Panel id="glossary" title={t("glossary.title", { n: total })} collapsed={collapsed} onToggle={onToggle} extra={
      <>
        <button className="primary" disabled={!pendingCount || !hasTranslation}
          title={t("glossary.updateTranslationTip")} onClick={onUpdateTranslation}>
          {t("glossary.updateTranslation")}{pendingCount ? ` (${pendingCount})` : ""}
        </button>
        <button onClick={() => setDialog({ term: null })}>{t("glossary.addTerm")}</button>
        <input placeholder={t("glossary.filter")} value={glossaryQuery}
          onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 140 }} />
        <button onClick={onRefreshGlossary} title={t("glossary.refresh")}>↻</button>
      </>
    }>
      {/* Doubles as the legend for the colour dots in the rows. */}
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
          <span />
          <span>{t("glossary.colCount")}</span>
          <span />
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
              <div
                key={term.source}
                className={`glossary-row ${pending[term.source] ? "dirty" : ""}`}
                onDoubleClick={() => setDialog({ term })}
              >
                <span className="glossary-cell" title={term.source}>{term.source}</span>
                <span className="glossary-cell" title={term.target}>{term.target}</span>
                {/* The kind is the dot alone; the filter row above names the colours. */}
                <span className={`kind-dot kind-${term.kind}`} title={t(`kind.${term.kind}`)} />
                <span className="glossary-count">
                  {term.frequency}{term.pinned ? " 📌" : ""}
                </span>
                <span className="glossary-actions">
                  <button className="ghost" title={t("glossary.edit")} onClick={() => setDialog({ term })}>✎</button>
                  <button className="ghost del" title={t("glossary.delete")} onClick={() => onDeleteTerm(term)}>✕</button>
                </span>
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

      {dialog && (
        <TermDialog
          t={t}
          term={dialog.term}
          hasTranslation={hasTranslation}
          onSave={(next) => onSaveTerm(next, dialog.term)}
          onClose={() => setDialog(null)}
        />
      )}
    </Panel>
  );
}
