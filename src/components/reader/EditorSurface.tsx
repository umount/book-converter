import { useEffect, useRef } from "react";
import type { Line } from "../../lib/highlight";

type Props = {
  lines: Line[];
  /** Term key whose occurrences are highlighted (IDE-style), or null. */
  activeKey: string | null;
  onTermClick?: (key: string, rect: DOMRect) => void;
  /** Global index of the active search match (scrolled into view), if searching. */
  currentSearch?: number;
};

/** Read-only editor surface: a line-number gutter plus tokenized text. */
export function EditorSurface({ lines, activeKey, onTermClick, currentSearch }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (currentSearch == null) return;
    const el = ref.current?.querySelector(".search-current");
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [currentSearch, lines]);

  return (
    <div className="editor-surface" ref={ref}>
      {lines.map((tokens, i) => (
        <div className="ed-line" key={i}>
          <span className="ed-gutter">{i + 1}</span>
          <span className="ed-content">
            {tokens.length === 0
              ? "​"
              : tokens.map((tk, j) => {
                  if (tk.search != null) {
                    return (
                      <span key={j} className={`search-hit ${tk.search === currentSearch ? "search-current" : ""}`}>
                        {tk.text}
                      </span>
                    );
                  }
                  if (tk.key) {
                    return (
                      <span
                        key={j}
                        className={`term ${tk.key === activeKey ? "term-active" : ""}`}
                        onClick={(e) => onTermClick?.(tk.key!, e.currentTarget.getBoundingClientRect())}
                      >
                        {tk.text}
                      </span>
                    );
                  }
                  return <span key={j}>{tk.text}</span>;
                })}
          </span>
        </div>
      ))}
    </div>
  );
}
