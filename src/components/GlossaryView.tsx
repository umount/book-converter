import { TERM_KINDS, type Progress, type Term } from "../types";
import { Panel } from "./Panel";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  collapsed: Record<string, boolean>;
  onToggle: (id: string) => void;
  glossary: Term[];
  filteredGlossary: Term[];
  glossaryQuery: string;
  setGlossaryQuery: (q: string) => void;
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

export function GlossaryView({
  t, collapsed, onToggle, glossary, filteredGlossary, glossaryQuery, setGlossaryQuery,
  newTerm, setNewTerm, pending, pendingCount, progress,
  onUpdateTranslation, onRefreshGlossary, onAddTerm,
  onRenameTerm, onEditTarget, onEditKind, onDeleteTerm,
}: Props) {
  return (
    <Panel id="glossary" title={t("glossary.title", { n: glossary.length })} collapsed={collapsed} onToggle={onToggle} extra={
      <>
        <button className="primary" disabled={!pendingCount || !(progress && progress.done > 0)}
          title={t("glossary.updateTranslationTip")} onClick={onUpdateTranslation}>
          {t("glossary.updateTranslation")}{pendingCount ? ` (${pendingCount})` : ""}
        </button>
        <input placeholder={t("glossary.filter")} value={glossaryQuery} onChange={(e) => setGlossaryQuery(e.target.value)} style={{ width: 140 }} />
        <button onClick={onRefreshGlossary}>↻</button>
      </>
    }>
      <div className="table-wrap">
        <table>
          <thead><tr><th>{t("glossary.colSource")}</th><th>{t("glossary.colTranslation")}</th><th>{t("glossary.colKind")}</th><th>{t("glossary.colCount")}</th><th>{t("glossary.colActions")}</th></tr></thead>
          <tbody>
            <tr className="add-row">
              <td><input placeholder={t("glossary.addSource")} value={newTerm.source} onChange={(e) => setNewTerm({ ...newTerm, source: e.target.value })} /></td>
              <td><input placeholder={t("glossary.addTranslation")} value={newTerm.target} onChange={(e) => setNewTerm({ ...newTerm, target: e.target.value })}
                onKeyDown={(e) => e.key === "Enter" && onAddTerm()} /></td>
              <td>
                <select value={newTerm.kind} onChange={(e) => setNewTerm({ ...newTerm, kind: e.target.value })}>
                  {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
                </select>
              </td>
              <td colSpan={2}><button onClick={onAddTerm} disabled={!newTerm.source.trim() || !newTerm.target.trim()}>{t("glossary.add")}</button></td>
            </tr>
            {filteredGlossary.map((term) => {
              const dirty = !!pending[term.source];
              return (
                <tr key={term.source} className={dirty ? "dirty" : ""}>
                  <td><input defaultValue={term.source} onBlur={(ev) => { const v = ev.target.value.trim(); if (v && v !== term.source) onRenameTerm(term, v); }} /></td>
                  <td><input key={term.target} defaultValue={term.target} onBlur={(ev) => onEditTarget(term, ev.target.value)} /></td>
                  <td>
                    <select value={term.kind} onChange={(ev) => onEditKind(term, ev.target.value)}>
                      {TERM_KINDS.map((k) => <option key={k} value={k}>{t(`kind.${k}`)}</option>)}
                    </select>
                  </td>
                  <td>{term.frequency}{term.pinned ? " 📌" : ""}</td>
                  <td><button className="ghost del" title={t("glossary.delete")} onClick={() => onDeleteTerm(term)}>✕</button></td>
                </tr>
              );
            })}
            {filteredGlossary.length === 0 && <tr><td colSpan={5} className="empty">{t("glossary.empty")}</td></tr>}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}
