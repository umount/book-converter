import { formatEta, langAbbr } from "../../lib/format";
import type { BookInfo, Progress } from "../../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  progress: Progress | null;
  book: BookInfo | null;
  glossaryCount: number;
  srcLang: string;
  tgtLang: string;
  busy: string | null;
  onToggleConsole: () => void;
};

/** Bottom status line: run state (left) and book/glossary context (right). */
export function StatusBar({
  t, progress, book, glossaryCount, srcLang, tgtLang, busy, onToggleConsole,
}: Props) {
  const running = !!progress?.running;
  const eta = running && progress?.eta_secs != null && progress.eta_secs > 0 ? formatEta(progress.eta_secs) : null;

  return (
    <footer className="statusbar">
      <div className="sb-item clickable" onClick={onToggleConsole} title={t("status.consoleTip")}>
        {busy ? (
          <><span className="spinner" /> <span className="sb-accent">{busy}</span></>
        ) : progress ? (
          <>
            <span className={`sb-dot ${running ? "running" : ""}`} />
            <span>{t("status.progress", { done: progress.done, total: progress.total })}</span>
            {progress.failed > 0 && <span className="sb-item">{t("progress.failed", { n: progress.failed })}</span>}
          </>
        ) : (
          <span>{t("status.idle")}</span>
        )}
      </div>
      {running && progress?.current_title && (
        <div className="sb-item" title={progress.current_title}>
          {progress.current_title.slice(0, 48)}
        </div>
      )}
      {eta && <div className="sb-item">{t("progress.eta", { eta })}</div>}

      <div className="sb-spacer" />

      {book && <div className="sb-item">{book.format.toUpperCase()}</div>}
      {book && <div className="sb-item">{book.encoding}</div>}
      <div className="sb-item" title={`${srcLang} → ${tgtLang}`}>{langAbbr(srcLang)}{"→"}{langAbbr(tgtLang)}</div>
      <div className="sb-item">{t("status.glossary", { n: glossaryCount })}</div>
    </footer>
  );
}
