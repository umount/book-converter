import { useEffect, useMemo, useRef, type MouseEvent } from "react";
import type { Line } from "../../lib/highlight";
import { LineTokens } from "./LineTokens";

type Props = {
  lines: Line[];
  /** Term key whose occurrences are highlighted (IDE-style), or null. */
  activeKey: string | null;
  onTermClick?: (key: string, rect: DOMRect) => void;
  /** Global index of the active search match (scrolled into view), if searching. */
  currentSearch?: number;
  /**
   * Type directly into the text. A transparent textarea is laid over the
   * highlighted lines, so the surface stays exactly the same (gutter, glossary
   * underlines, search hits) instead of switching into an edit mode.
   */
  editable?: boolean;
  /** Current text, required when `editable`; must match `lines`. */
  value?: string;
  onChange?: (next: string) => void;
};

/** Editor surface: a line-number gutter plus tokenized, optionally editable text. */
export function EditorSurface({
  lines, activeKey, onTermClick, currentSearch, editable, value, onChange,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (currentSearch == null) return;
    const el = ref.current?.querySelector(".search-current");
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [currentSearch, lines]);

  // Character offset where each line starts, so a caret position in the overlay
  // maps back to the token under it (the tokens themselves are not clickable
  // while editing: the textarea is on top).
  const lineStarts = useMemo(() => {
    const starts: number[] = [];
    let at = 0;
    for (const tokens of lines) {
      starts.push(at);
      at += tokens.reduce((n, tk) => n + tk.text.length, 0) + 1; // + "\n"
    }
    return starts;
  }, [lines]);

  function keyAtOffset(pos: number): string | null {
    let li = 0;
    while (li + 1 < lineStarts.length && lineStarts[li + 1] <= pos) li++;
    let at = lineStarts[li] ?? 0;
    for (const tk of lines[li] ?? []) {
      const end = at + tk.text.length;
      if (pos >= at && pos < end) return tk.key ?? null;
      at = end;
    }
    return null;
  }

  // Clicking a glossary term still opens its popover; the caret lands there too.
  function onOverlayClick(e: MouseEvent<HTMLTextAreaElement>) {
    if (!onTermClick) return;
    const key = keyAtOffset(e.currentTarget.selectionStart);
    if (key) onTermClick(key, new DOMRect(e.clientX, e.clientY, 1, 4));
  }

  return (
    <div className={`editor-surface ${editable ? "editable" : ""}`} ref={ref}>
      <div className="ed-lines">
        {lines.map((tokens, i) => (
          <div className="ed-line" key={i}>
            <span className="ed-gutter">{i + 1}</span>
            <span className="ed-content">
              <LineTokens
                tokens={tokens} activeKey={activeKey}
                onTermClick={onTermClick} currentSearch={currentSearch}
              />
            </span>
          </div>
        ))}
      </div>
      {editable && (
        <textarea
          className="ed-input"
          value={value ?? ""}
          onChange={(e) => onChange?.(e.target.value)}
          onClick={onOverlayClick}
          spellCheck={false}
        />
      )}
    </div>
  );
}
