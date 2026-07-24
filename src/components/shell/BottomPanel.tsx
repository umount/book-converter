import { useRef, useState, type ReactNode } from "react";

const LS_HEIGHT = "bc.console.height";
const MIN = 90;
const MAX = 600;

type Props = { children: ReactNode };

/** Resizable host for the console. Drag the top edge to resize; height persists. */
export function BottomPanel({ children }: Props) {
  const [height, setHeight] = useState<number>(() => {
    const v = Number(localStorage.getItem(LS_HEIGHT));
    return v >= MIN && v <= MAX ? v : 180;
  });
  const [dragging, setDragging] = useState(false);
  const startY = useRef(0);
  const startH = useRef(0);

  function onPointerDown(e: React.PointerEvent) {
    e.preventDefault();
    startY.current = e.clientY;
    startH.current = height;
    setDragging(true);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: React.PointerEvent) {
    if (!dragging) return;
    const next = Math.min(MAX, Math.max(MIN, startH.current + (startY.current - e.clientY)));
    setHeight(next);
  }
  function onPointerUp(e: React.PointerEvent) {
    if (!dragging) return;
    setDragging(false);
    localStorage.setItem(LS_HEIGHT, String(height));
    (e.target as HTMLElement).releasePointerCapture(e.pointerId);
  }

  return (
    <section className="bottompanel" style={{ height }}>
      <div
        className={`resize-handle ${dragging ? "dragging" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      />
      {children}
    </section>
  );
}
