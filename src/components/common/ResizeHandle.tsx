import { useRef, useState, type PointerEvent } from "react";

type Props = {
  /** "x" for a vertical divider dragged sideways, "y" for a horizontal one. */
  axis: "x" | "y";
  /** Drag started: capture whatever size the caller is about to change. */
  onStart?: () => void;
  /** Pixels moved since the drag started (signed, along `axis`). */
  onDrag: (delta: number) => void;
  /** Drag finished: persist the new size. */
  onEnd?: () => void;
  className?: string;
};

/** Draggable splitter. Owns the pointer bookkeeping; the caller owns the size. */
export function ResizeHandle({ axis, onStart, onDrag, onEnd, className = "" }: Props) {
  const [dragging, setDragging] = useState(false);
  const origin = useRef(0);
  const at = (e: PointerEvent) => (axis === "x" ? e.clientX : e.clientY);

  function onPointerDown(e: PointerEvent) {
    e.preventDefault();
    origin.current = at(e);
    setDragging(true);
    onStart?.();
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (dragging) onDrag(at(e) - origin.current);
  }
  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    setDragging(false);
    onEnd?.();
    (e.target as HTMLElement).releasePointerCapture(e.pointerId);
  }

  return (
    <div
      className={`resize-handle ${axis === "x" ? "vertical" : "horizontal"} ${dragging ? "dragging" : ""} ${className}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    />
  );
}
