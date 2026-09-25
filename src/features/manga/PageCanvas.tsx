import { useEffect, useRef, useState } from "react";
import type {
  MangaRegionView,
  PixelBounds,
  PageSummary,
} from "../../shared/contracts/generated";
import { assetUrl } from "../../shared/api/assets";
import {
  fittedWidth,
  pageKeyDelta,
  dragRegion,
} from "../../shared/state/mangaCanvas";
import type { T } from "../../app/strings";

export function PageCanvas({
  projectId,
  renderedAssetId,
  page,
  zoom,
  rtl,
  loading,
  onNavigate,
  regions = [],
  selectedRegion,
  onSelectRegion,
  onChangeBounds,
  t,
}: {
  projectId: string;
  renderedAssetId?: string | null;
  page?: PageSummary;
  zoom: string;
  rtl: boolean;
  loading: boolean;
  onNavigate: (delta: number) => void;
  regions?: MangaRegionView[];
  selectedRegion?: string | null;
  onSelectRegion?: (id: string) => void;
  onChangeBounds?: (id: string, bounds: PixelBounds) => void;
  t: T;
}) {
  const regionDrag = useRef<{
    id: string;
    x: number;
    y: number;
    bounds: PixelBounds;
    resize: boolean;
  } | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const drag = useRef<{
    x: number;
    y: number;
    left: number;
    top: number;
  } | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() =>
      setSize({ width: element.clientWidth, height: element.clientHeight }),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    drag.current = null;
    regionDrag.current = null;
    viewport.current?.scrollTo(0, 0);
  }, [page?.id, zoom]);
  const width = page
    ? fittedWidth(page.width, page.height, size.width, size.height, zoom)
    : 0;
  return (
    <div
      className="bc-page-viewport"
      ref={viewport}
      tabIndex={0}
      aria-label={t("pages")}
      onKeyDown={(e) => {
        if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
        const delta = pageKeyDelta(e.key, rtl);
        if (delta) {
          e.preventDefault();
          onNavigate(delta);
        }
      }}
      onPointerDown={(e) => {
        if (e.button !== 0 || !viewport.current) return;
        e.currentTarget.focus();
        drag.current = {
          x: e.clientX,
          y: e.clientY,
          left: viewport.current.scrollLeft,
          top: viewport.current.scrollTop,
        };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (!drag.current || !viewport.current) return;
        viewport.current.scrollLeft =
          drag.current.left + drag.current.x - e.clientX;
        viewport.current.scrollTop =
          drag.current.top + drag.current.y - e.clientY;
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onLostPointerCapture={() => {
        drag.current = null;
      }}
    >
      {page ? (
        <div
          key={page.id}
          style={{ position: "relative", width, margin: "0 auto" }}
        >
          <img
            draggable={false}
            src={assetUrl(projectId, renderedAssetId ?? page.originalAssetId)}
            alt={`${t("page")} ${page.position + 1}`}
            style={{ display: "block", width: "100%", height: "auto" }}
          />
          {regions.map((region) => (
            <button
              key={region.id}
              className="bc-region-overlay"
              aria-label={`${t("region")} ${region.readingOrder + 1}: ${region.sourceText}`}
              aria-pressed={region.id === selectedRegion}
              onPointerDown={(e) => {
                e.stopPropagation();
                if (e.button !== 0 || !onChangeBounds || !width) return;
                onSelectRegion?.(region.id);
                regionDrag.current = {
                  id: region.id,
                  x: e.clientX,
                  y: e.clientY,
                  bounds: { ...region.bounds },
                  resize: (e.target as HTMLElement).dataset.resize === "true",
                };
                e.currentTarget.setPointerCapture(e.pointerId);
              }}
              onPointerMove={(e) => {
                const d = regionDrag.current;
                if (!d || d.id !== region.id || !onChangeBounds || !width)
                  return;
                const b = dragRegion(
                  d.bounds,
                  e.clientX - d.x,
                  e.clientY - d.y,
                  d.resize,
                  page.width,
                  page.height,
                  width,
                );
                onChangeBounds(region.id, b);
              }}
              onPointerUp={() => {
                regionDrag.current = null;
              }}
              onPointerCancel={() => {
                regionDrag.current = null;
              }}
              onLostPointerCapture={() => {
                regionDrag.current = null;
              }}
              onClick={() => onSelectRegion?.(region.id)}
              style={{
                left: `${(region.bounds.x / page.width) * 100}%`,
                top: `${(region.bounds.y / page.height) * 100}%`,
                width: `${(region.bounds.width / page.width) * 100}%`,
                height: `${(region.bounds.height / page.height) * 100}%`,
              }}
            >
              <span>{region.readingOrder + 1}</span>
              {onChangeBounds && (
                <span
                  data-resize="true"
                  className="bc-region-resize"
                  aria-hidden="true"
                >
                  ↘
                </span>
              )}
            </button>
          ))}
        </div>
      ) : (
        <p>{t(loading ? "loading" : "noPages")}</p>
      )}
    </div>
  );
}
