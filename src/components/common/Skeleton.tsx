type Props = {
  /** Number of placeholder rows to draw. */
  rows?: number;
  /** Row height in pixels; match the real row so nothing jumps on load. */
  rowHeight?: number;
  className?: string;
};

/** Shimmering placeholder rows shown while a list is loading. */
export function SkeletonList({ rows = 8, rowHeight = 26, className = "" }: Props) {
  // Varying widths read as content rather than as a solid block.
  const widths = [82, 64, 91, 55, 76, 88, 60, 84];
  return (
    <div className={`skeleton-list ${className}`} aria-busy="true">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="skeleton-row" style={{ height: rowHeight }}>
          <span className="skeleton-bar" style={{ width: `${widths[i % widths.length]}%` }} />
        </div>
      ))}
    </div>
  );
}
