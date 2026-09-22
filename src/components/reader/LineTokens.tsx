import type { Line } from "../../lib/highlight";

type Props = {
  tokens: Line;
  /** Term key whose occurrences are highlighted, or null. */
  activeKey: string | null;
  onTermClick?: (key: string, rect: DOMRect) => void;
  /** Global index of the active search match, if searching. */
  currentSearch?: number;
};

/**
 * One tokenized line: glossary terms underlined and clickable, search hits
 * marked. Shared by the editor surface and the page surface so a chapter's text
 * looks and behaves the same whether or not the page also holds pictures.
 */
export function LineTokens({ tokens, activeKey, onTermClick, currentSearch }: Props) {
  if (tokens.length === 0) return <>&#8203;</>;
  return (
    <>
      {tokens.map((tk, j) => {
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
    </>
  );
}
