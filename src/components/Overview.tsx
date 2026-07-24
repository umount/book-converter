import { formatEta } from "../lib/format";
import type { BookDetails, Progress, RefInfo } from "../types";
import { Panel } from "./Panel";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  collapsed: Record<string, boolean>;
  onToggle: (id: string) => void;
  refInfo: RefInfo | null;
  details: BookDetails | null;
  progress: Progress | null;
  activeKey: number;
  sample: number;
  setSample: (n: number) => void;
  limit: number | "";
  setLimit: (v: number | "") => void;
  reFrom: number;
  setReFrom: (n: number) => void;
  onTranslateTitle: () => void;
  onReplaceCover: () => void;
  onGenerateSummary: () => void;
  onSaveSummary: (text: string) => void;
  onOpenReference: () => void;
  onBootstrap: () => void;
  onHarvestGlossary: (fromEnd: boolean) => void;
  onStart: () => void;
  onPause: () => void;
  onRefreshProgress: () => void;
  onRetranslate: (pos: number | null) => void;
};

export function Overview({
  t, collapsed, onToggle, refInfo, details, progress, activeKey,
  sample, setSample, limit, setLimit, reFrom, setReFrom,
  onTranslateTitle, onReplaceCover, onGenerateSummary, onSaveSummary,
  onOpenReference, onBootstrap, onHarvestGlossary, onStart, onPause, onRefreshProgress, onRetranslate,
}: Props) {
  const jobTotal = progress?.job_total && progress.job_total > 0 ? progress.job_total : null;
  const jobDone = progress?.job_done ?? 0;
  const showJob = !!(progress?.running && jobTotal);
  const pct = showJob
    ? Math.round((jobDone / jobTotal!) * 100)
    : progress && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : 0;
  const barLabel = showJob
    ? `${jobDone}/${jobTotal}`
    : progress
      ? `${progress.done}/${progress.total}`
      : "";
  const eta =
    progress?.running && progress.eta_secs != null && progress.eta_secs > 0
      ? formatEta(progress.eta_secs)
      : null;
  const current =
    progress?.running && progress.current_title
      ? progress.current_title.slice(0, 50)
      : null;

  return (
    <>
      {progress && (
        <div className="stats">
          <div className="stat"><div className="stat-n">{progress.total}</div><div className="stat-l">{t("overview.statTotal")}</div></div>
          <div className="stat"><div className="stat-n">{progress.done}</div><div className="stat-l">{t("overview.statDone")}</div></div>
          <div className="stat"><div className="stat-n">{progress.pending}</div><div className="stat-l">{t("overview.statPending")}</div></div>
          <div className={`stat ${progress.failed > 0 ? "failed" : ""}`}><div className="stat-n">{progress.failed}</div><div className="stat-l">{t("overview.statFailed")}</div></div>
        </div>
      )}

      <Panel id="book" title={t("panel.book")} collapsed={collapsed} onToggle={onToggle}>
        <div className="book-details">
          {details?.cover ? <img className="cover" src={details.cover} alt="cover" /> : <div className="cover cover-empty">{t("book.noCover")}</div>}
          <div className="book-meta">
            <div className="row">
              <strong className="book-title">{details?.title_translated || details?.title || t("book.untitled")}</strong>
              {details && !details.title_translated && <button onClick={onTranslateTitle}>{t("book.translateTitle")}</button>}
              <button onClick={onReplaceCover}>{t("book.replaceCover")}</button>
            </div>
            {details?.title_translated && details?.title && <div className="muted">{t("book.original", { title: details.title })}</div>}
            <div className="muted">{details?.author_translated || details?.author}</div>
            <div className="summary-head">
              {details?.title && details?.author && (
                <button className="ghost" title={t("book.generateSummaryTip")} onClick={onGenerateSummary}>✨ {t("book.generateSummary")}</button>
              )}
            </div>
            <textarea className="summary" placeholder={t("book.summaryPlaceholder")}
              key={activeKey + (details?.summary ?? "")} defaultValue={details?.summary || ""}
              onBlur={(e) => onSaveSummary(e.target.value)} />
          </div>
        </div>
      </Panel>

      <Panel id="reference" title={t("panel.reference")} collapsed={collapsed} onToggle={onToggle}>
        {!refInfo ? (
          <div className="ref-empty">
            <p className="muted ref-desc">{t("reference.description")}</p>
            <button onClick={onOpenReference}>{t("reference.add")}</button>
          </div>
        ) : (
          <div className="row">
            <strong>{refInfo.title || t("book.untitled")}</strong>
            <span className="muted">{t("reference.covers", { n: refInfo.max_covered ?? "?", chapters: refInfo.chapters })}</span>
            <div className="menu-spacer" />
            <button className="ghost" onClick={onOpenReference}>{t("reference.replace")}</button>
          </div>
        )}
        {(refInfo || (progress && progress.done > 0)) && (
          <div className="retranslate" style={{ marginTop: 10 }}>
            <div className="row">
              <label>{t("reference.bootstrapLabel")}</label>
              <input
                type="number"
                min={1}
                value={sample}
                onChange={(e) => setSample(Math.max(1, Number(e.target.value) || 1))}
                style={{ width: 64 }}
                title={t("glossary.harvestSampleTip")}
              />
              {refInfo && (
                <button disabled={!!progress?.running} onClick={onBootstrap} title={t("translate.bootstrap")}>
                  {t("translate.bootstrap")}
                </button>
              )}
              {progress && progress.done > 0 && (
                <>
                  <button
                    className="ghost"
                    disabled={progress.running}
                    onClick={() => onHarvestGlossary(false)}
                    title={t("glossary.harvestHint")}
                  >
                    {t("glossary.harvestStart")}
                  </button>
                  <button
                    className="ghost"
                    disabled={progress.running}
                    onClick={() => onHarvestGlossary(true)}
                    title={t("glossary.harvestHint")}
                  >
                    {t("glossary.harvestEnd")}
                  </button>
                </>
              )}
            </div>
            <div className="muted resume-hint">
              {refInfo && progress && progress.done > 0
                ? t("glossary.seedHintBoth")
                : refInfo
                  ? t("glossary.seedHintReference")
                  : t("glossary.harvestHint")}
            </div>
          </div>
        )}
      </Panel>

      <Panel id="translate" title={t("panel.translate")} collapsed={collapsed} onToggle={onToggle}>
        <div className="row">
          <label>{t("translate.next")}</label>
          <input type="number" min={1} placeholder={t("translate.allRemaining")} value={limit} onChange={(e) => setLimit(e.target.value === "" ? "" : Number(e.target.value))} style={{ width: 120 }} />
          <span className="muted">{t("translate.chapters")}</span>
          <button onClick={onStart} disabled={progress?.running}>{t("translate.start")}</button>
          <button onClick={onPause} disabled={!progress?.running}>{t("translate.pause")}</button>
          <button className="ghost" onClick={onRefreshProgress}>↻</button>
        </div>
        {progress && (
          <div className="muted resume-hint">
            {progress.pending > 0
              ? t("translate.resumeHint", {
                  from: progress.next_number ?? "?",
                  remaining: progress.pending,
                })
              : t("translate.resumeHintAllDone")}
          </div>
        )}
        {progress && (
          <div className="progress">
            <div className="bar"><div className="bar-fill" style={{ width: `${pct}%` }} /></div>
            <div className="progress-text">
              {barLabel} ({pct}%)
              {progress.failed > 0 && ` · ${t("progress.failed", { n: progress.failed })}`}
              {progress.running ? ` · ${t("progress.running")}` : ""}
              {eta ? ` · ${t("progress.eta", { eta })}` : ""}
              {current ? ` · ${t("progress.current", { title: current })}` : ""}
            </div>
          </div>
        )}
        {progress && progress.done > 0 && (
          <div className="retranslate">
            <div className="row">
              <span className="muted">{t("translate.retranslate")}:</span>
              <button className="ghost danger" disabled={progress.running} onClick={() => onRetranslate(null)}>{t("translate.retranslateAll")}</button>
              <button className="ghost danger" disabled={progress.running} onClick={() => onRetranslate(reFrom)}>{t("translate.retranslateFrom")}</button>
              <input type="number" min={1} max={progress.max_number ?? progress.total} value={reFrom}
                onChange={(e) => setReFrom(Math.max(1, Number(e.target.value) || 1))} style={{ width: 80 }}
                title={t("translate.retranslateNumberTip")} />
            </div>
            <div className="muted resume-hint">{t("translate.retranslateHint")}</div>
          </div>
        )}
      </Panel>
    </>
  );
}
