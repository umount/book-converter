import type { RefObject } from "react";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  log: string[];
  logRef: RefObject<HTMLDivElement>;
  onClear: () => void;
  onClose: () => void;
};

export function Console({ t, log, logRef, onClear, onClose }: Props) {
  return (
    <section className="console">
      <div className="console-head">
        <span className="console-title">{t("console.title")}</span>
        <div className="menu-spacer" />
        <button className="icon" title={t("console.clear")} onClick={onClear}>⌫</button>
        <button className="icon" onClick={onClose}>×</button>
      </div>
      <div className="log" ref={logRef}>
        {log.length === 0
          ? <div className="muted">{t("console.empty")}</div>
          : log.map((l, i) => <div key={i}>{l}</div>)}
      </div>
    </section>
  );
}
