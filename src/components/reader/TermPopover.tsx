import { useEffect } from "react";
import type { Term } from "../../types";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  term: Term;
  rect: DOMRect;
  onClose: () => void;
  onOpenGlossary: (source: string) => void;
};

/** Small popover shown when a glossary term is clicked in the reader. */
export function TermPopover({ t, term, rect, onClose, onOpenGlossary }: Props) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const style: React.CSSProperties = {
    position: "fixed",
    top: Math.min(rect.bottom + 6, window.innerHeight - 110),
    left: Math.min(rect.left, window.innerWidth - 300),
  };

  return (
    <>
      <div className="popover-backdrop" onClick={onClose} />
      <div className="term-popover" style={style} onClick={(e) => e.stopPropagation()}>
        <div className="tp-row">
          <span className="tp-source">{term.source}</span>
          <span className="tp-arrow">→</span>
          <span className="tp-target">{term.target}</span>
          <span className="tp-kind">{t(`kind.${term.kind}`)}</span>
        </div>
        <button className="ghost tp-open" onClick={() => { onOpenGlossary(term.source); onClose(); }}>
          {t("reader.openInGlossary")}
        </button>
      </div>
    </>
  );
}
