import { useRef, useState, type ReactNode } from "react";

/** A panel edge supports both pointer dragging and keyboard resizing. */
export function ResizablePanel({
  children,
  className,
  label,
  edge,
  hidden = false,
  minWidth = 150,
}: {
  children: ReactNode;
  className: string;
  label: string;
  edge: "left" | "right";
  hidden?: boolean;
  minWidth?: number;
}) {
  const panel = useRef<HTMLElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [width, setWidth] = useState<number>();
  const direction = edge === "right" ? 1 : -1;
  const resize = (value: number) =>
    setWidth(Math.max(minWidth, Math.min(window.innerWidth * 0.45, value)));
  return (
    <aside
      ref={panel}
      className={`${className} bc-resizable-panel`}
      hidden={hidden}
      aria-label={label}
      style={width === undefined ? undefined : { width, flexBasis: width }}
    >
      {children}
      <div
        className={`bc-panel-resizer bc-panel-resizer-${edge}`}
        role="separator"
        tabIndex={0}
        aria-label={label}
        aria-orientation="vertical"
        aria-valuemin={minWidth}
        aria-valuenow={Math.round(width ?? (edge === "left" ? 360 : 250))}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.preventDefault();
          e.currentTarget.focus();
          drag.current = {
            x: e.clientX,
            width: panel.current!.getBoundingClientRect().width,
          };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (drag.current)
            resize(
              drag.current.width + direction * (e.clientX - drag.current.x),
            );
        }}
        onPointerUp={(e) => {
          drag.current = null;
          if (e.currentTarget.hasPointerCapture(e.pointerId))
            e.currentTarget.releasePointerCapture(e.pointerId);
        }}
        onLostPointerCapture={() => {
          drag.current = null;
        }}
        onKeyDown={(e) => {
          if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
          e.preventDefault();
          resize(
            panel.current!.getBoundingClientRect().width +
              direction * (e.key === "ArrowRight" ? 16 : -16),
          );
        }}
      />
    </aside>
  );
}
