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
