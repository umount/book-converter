import { useEffect, useRef } from "react";

type Handler = (e: KeyboardEvent) => void;

/** Normalize a keyboard event to a combo string like "mod+p" or "alt+arrowdown". */
function comboOf(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("mod");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  parts.push(e.key.toLowerCase());
  return parts.join("+");
}

/**
 * Global keyboard shortcuts. Keys are combo strings ("mod" = Cmd or Ctrl).
 * Handlers decide whether to `preventDefault`. The latest `map` is always used,
 * so callers can pass fresh closures without re-subscribing.
 */
export function useHotkeys(map: Record<string, Handler>) {
  const ref = useRef(map);
  ref.current = map;
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const h = ref.current[comboOf(e)];
      if (h) h(e);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
