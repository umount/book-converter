import { useEffect, useRef, useState, type ReactNode } from "react";

type Props<T> = {
  items: T[];
  /** Fixed row height in px; every rendered row must be exactly this tall. */
  rowHeight: number;
  renderRow: (item: T, index: number) => ReactNode;
  overscan?: number;
  className?: string;
};

/**
 * Generic fixed-height windowing list: renders only the rows in view (plus a
 * small overscan), so lists of thousands of rows stay cheap. No dependency.
 */
export function VirtualList<T>({ items, rowHeight, renderRow, overscan = 6, className }: Props<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(0);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    setHeight(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  const total = items.length * rowHeight;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(items.length, start + Math.ceil(height / rowHeight) + overscan * 2);
  const slice = items.slice(start, end);

  return (
    <div
      ref={ref}
      className={className}
      onScroll={(e) => setScrollTop((e.target as HTMLDivElement).scrollTop)}
      style={{ overflowY: "auto", position: "relative" }}
    >
      <div style={{ height: total, position: "relative" }}>
        <div style={{ position: "absolute", top: start * rowHeight, left: 0, right: 0 }}>
          {slice.map((item, i) => renderRow(item, start + i))}
        </div>
      </div>
    </div>
  );
}
