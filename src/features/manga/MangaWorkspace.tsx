import { PageCanvas } from "./PageCanvas";
import { VirtualList } from "../../shared/ui/VirtualList";
import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  MangaVolumeSummary,
  PageSummary,
} from "../../shared/contracts/generated";
import { assetUrl } from "../../shared/api/assets";
import { errorText, type T } from "../../app/strings";
export function MangaWorkspace({ projectId, t }: { projectId: string; t: T }) {
  return <MangaProjectWorkspace key={projectId} projectId={projectId} t={t} />;
}

function MangaProjectWorkspace({ projectId, t }: { projectId: string; t: T }) {
  const [volumes, setVolumes] = useState<MangaVolumeSummary[]>([]);
  const [volumeId, setVolumeId] = useState("");
  const [loading, setLoading] = useState(true);
  const [pages, setPages] = useState<PageSummary[]>([]),
    [selected, setSelected] = useState(0),
    [error, setError] = useState<unknown>(null);
  const [zoom, setZoom] = useState("fit");
  useEffect(() => {
    let alive = true;
    projectApi
      .mangaVolumes({ projectId })
      .then((items) => {
        if (alive) setVolumes(items);
      })
      .catch((e) => {
        if (alive) setError(e);
      });
    return () => {
      alive = false;
    };
  }, [projectId]);
  useEffect(() => {
    let alive = true;
    setPages([]);
    setSelected(0);
    setError(null);
    setLoading(true);
    void (async () => {
      let cursor: string | null = null;
      const all: PageSummary[] = [];
      do {
        const result = await projectApi.mangaPages({
          projectId,
          volumeId: volumeId || null,
          cursor,
          limit: 200,
        });
        if (!alive) return;
        all.push(...result.items);
        cursor = result.nextCursor;
        setPages([...all]);
      } while (cursor);
      if (alive) setLoading(false);
    })().catch((e) => {
      if (alive) {
        setError(e);
        setLoading(false);
      }
    });
    return () => {
      alive = false;
    };
  }, [projectId, volumeId]);
  const page = pages[selected];
  return (
    <div className="bc-manga">
      <aside>
        <select
          aria-label={t("volumes")}
          value={volumeId}
          onChange={(e) => setVolumeId(e.target.value)}
        >
          <option value="">{t("allVolumes")}</option>
          {volumes.map((v) => (
            <option key={v.id} value={v.id}>
              {v.title} ({v.pageCount})
            </option>
          ))}
        </select>
        <VirtualList
          items={pages}
          activeIndex={selected}
          rowHeight={156}
          className="bc-page-list"
          overscan={2}
          renderRow={(p, i) => (
            <button
              style={{ height: 156, margin: 0 }}
              key={p.id}
              aria-current={i === selected ? "page" : undefined}
              onClick={() => setSelected(i)}
            >
              <img
                loading="lazy"
                src={assetUrl(
                  projectId,
                  p.thumbnailAssetId ?? p.originalAssetId,
                )}
                alt=""
              />
              <span>
                {t("page")} {p.position + 1}
                {!volumeId && volumes.length > 1 && (
                  <small>
                    {volumes.find((v) => v.id === p.volumeId)?.title}
                  </small>
                )}
              </span>
            </button>
          )}
        />
      </aside>
      <div className="bc-manga-page">
        <p className="bc-warning">{t("mangaUnavailable")}</p>
        {error != null && (
          <p role="alert" className="bc-error">
            {errorText(error, t)}
          </p>
        )}
        <div className="bc-toolbar">
          <button
            disabled={selected === 0 || !page}
            onClick={() => setSelected((i) => Math.max(0, i - 1))}
            aria-label={t("previousPage")}
          >
            ←
          </button>
          <label>
            {t("page")}{" "}
            <input
              type="number"
              min={1}
              max={pages.length}
              value={pages.length ? selected + 1 : 0}
              style={{ width: 80 }}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isInteger(n) && n >= 1 && n <= pages.length)
                  setSelected(n - 1);
              }}
            />{" "}
            / {pages.length}
          </label>
          <button
            disabled={selected >= pages.length - 1}
            onClick={() =>
              setSelected((i) => Math.min(pages.length - 1, i + 1))
            }
            aria-label={t("nextPage")}
          >
            →
          </button>
          <label>
            {t("zoom")}{" "}
            <select value={zoom} onChange={(e) => setZoom(e.target.value)}>
              <option value="fit">{t("fitPage")}</option>
              <option value="page">{t("wholePage")}</option>
              {[25, 50, 75, 100, 150, 200, 300].map((n) => (
                <option key={n} value={n}>
                  {n}%
                </option>
              ))}
            </select>
          </label>
        </div>
        <PageCanvas
          projectId={projectId}
          page={page}
          zoom={zoom}
          loading={loading}
          rtl={
            volumes.find((v) => v.id === page?.volumeId)?.readingDirection ===
            "rtl"
          }
          onNavigate={(delta) =>
            setSelected((i) =>
              Math.max(0, Math.min(pages.length - 1, i + delta)),
            )
          }
          t={t}
        />
      </div>
    </div>
  );
}
