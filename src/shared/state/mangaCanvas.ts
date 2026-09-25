/** Canonical image pixels stay independent of viewport size and reading direction. */
export function fittedWidth(
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
  mode: string,
): number {
  if (width <= 0 || height <= 0 || viewportWidth <= 0 || viewportHeight <= 0)
    return 0;
  if (mode === "page")
    return Math.min(viewportWidth, (viewportHeight * width) / height);
  if (mode === "fit") return viewportWidth;
  const percent = Number(mode);
  return Number.isFinite(percent) && percent > 0
    ? (width * percent) / 100
    : viewportWidth;
}

export function pageKeyDelta(key: string, rtl: boolean): number {
  if (key === "PageDown") return 1;
  if (key === "PageUp") return -1;
  if (key === "ArrowLeft") return rtl ? 1 : -1;
  if (key === "ArrowRight") return rtl ? -1 : 1;
  return 0;
}

/** Convert viewport movement back to image pixels and keep edits inside the page. */
export function dragRegion(
  bounds: { x: number; y: number; width: number; height: number },
  dx: number,
  dy: number,
  resize: boolean,
  pageWidth: number,
  pageHeight: number,
  displayedWidth: number,
) {
  const b = { ...bounds };
  if (displayedWidth <= 0) return b;
  dx = (dx * pageWidth) / displayedWidth;
  dy = (dy * pageWidth) / displayedWidth;
  if (resize) {
    b.width = Math.min(pageWidth - b.x, Math.max(8, b.width + dx));
    b.height = Math.min(pageHeight - b.y, Math.max(8, b.height + dy));
  } else {
    b.x = Math.max(0, Math.min(pageWidth - b.width, b.x + dx));
    b.y = Math.max(0, Math.min(pageHeight - b.height, b.y + dy));
  }
  return b;
}
