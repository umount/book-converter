import { useRef, useState, type ReactNode } from "react";
import { ResizeHandle } from "../common/ResizeHandle";

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
  const startH = useRef(height);

  return (
    <section className="bottompanel" style={{ height }}>
      <ResizeHandle
        axis="y"
        onStart={() => { startH.current = height; }}
        // Dragging up (negative dy) makes the panel taller.
        onDrag={(dy) => setHeight(Math.min(MAX, Math.max(MIN, startH.current - dy)))}
        onEnd={() => localStorage.setItem(LS_HEIGHT, String(height))}
      />
      {children}
    </section>
  );
}
