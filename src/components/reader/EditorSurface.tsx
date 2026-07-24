import type { Line } from "../../lib/highlight";

type Props = {
  lines: Line[];
  /** Term key whose occurrences are highlighted (IDE-style), or null. */
  activeKey: string | null;
  onTermClick?: (key: string, rect: DOMRect) => void;
};

/** Read-only editor surface: a line-number gutter plus tokenized text. */
export function EditorSurface({ lines, activeKey, onTermClick }: Props) {
  return (
    <div className="editor-surface">
      {lines.map((tokens, i) => (
        <div className="ed-line" key={i}>
          <span className="ed-gutter">{i + 1}</span>
          <span className="ed-content">
            {tokens.length === 0
              ? "​"
              : tokens.map((tk, j) =>
                  tk.key ? (
                    <span
                      key={j}
                      className={`term ${tk.key === activeKey ? "term-active" : ""}`}
                      onClick={(e) => onTermClick?.(tk.key!, e.currentTarget.getBoundingClientRect())}
                    >
                      {tk.text}
                    </span>
                  ) : (
                    <span key={j}>{tk.text}</span>
                  ),
                )}
          </span>
        </div>
      ))}
    </div>
  );
}
