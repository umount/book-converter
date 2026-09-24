import { useEffect, useRef, useState } from "react";
import type { PageSummary } from "../../shared/contracts/generated";
import { assetUrl } from "../../shared/api/assets";
import { fittedWidth, pageKeyDelta } from "../../shared/state/mangaCanvas";
import type { T } from "../../app/strings";

export function PageCanvas({
  projectId,
  page,
  zoom,
  rtl,
  loading,
  onNavigate,
  t,
}: {
  projectId: string;
  page?: PageSummary;
  zoom: string;
  rtl: boolean;
  loading: boolean;
  onNavigate: (delta: number) => void;
  t: T;
}) {
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
        <img
          key={page.id}
          draggable={false}
          src={assetUrl(projectId, page.originalAssetId)}
          alt={`${t("page")} ${page.position + 1}`}
          style={{
            display: "block",
            margin: "0 auto",
            width,
            maxWidth: "none",
            height: "auto",
          }}
        />
      ) : (
        <p>{t(loading ? "loading" : "noPages")}</p>
      )}
    </div>
  );
}
