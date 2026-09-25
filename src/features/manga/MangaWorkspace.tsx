import { activePageRebuild } from "../../shared/state/mangaCanvas";
import { ProcessingStatus } from "./ProcessingStatus";
import { RegionInspector } from "./RegionInspector";
import { usePageView } from "./usePageView";
import { PageCanvas } from "./PageCanvas";
import { VirtualList } from "../../shared/ui/VirtualList";
import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  MangaVolumeSummary,
  PageSummary,
  MangaRegionView,
  JobView,
  JobRef,
} from "../../shared/contracts/generated";
import { assetUrl } from "../../shared/api/assets";
import { errorText, type T } from "../../app/strings";
export function MangaWorkspace({
  projectId,
  t,
  onSettings,
  setupVersion,
  batch = false,
  jobs,
  onJob,
}: {
  projectId: string;
  t: T;
  onSettings: () => void;
  setupVersion: number;
  batch?: boolean;
  jobs: JobView[];
  onJob: (job: JobRef) => Promise<void>;
}) {
  return (
    <MangaProjectWorkspace
      key={projectId}
      projectId={projectId}
      t={t}
      onSettings={onSettings}
      setupVersion={setupVersion}
      batch={batch}
      jobs={jobs}
      onJob={onJob}
    />
  );
}

function MangaProjectWorkspace({
  projectId,
  t,
  onSettings,
  setupVersion,
  batch = false,
  jobs,
  onJob,
}: {
  projectId: string;
  t: T;
  onSettings: () => void;
  setupVersion: number;
  batch?: boolean;
  jobs: JobView[];
  onJob: (job: JobRef) => Promise<void>;
}) {
  const [volumes, setVolumes] = useState<MangaVolumeSummary[]>([]);
  const [volumeId, setVolumeId] = useState("");
  const [loading, setLoading] = useState(true);
  const [pages, setPages] = useState<PageSummary[]>([]),
    [selected, setSelected] = useState(0),
    [error, setError] = useState<unknown>(null);
  const [showTranslation, setShowTranslation] = useState(true);
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
  const { view, error: pageError, refresh } = usePageView(projectId, page?.id);
  const [edits, setEdits] = useState<
    Record<string, Partial<Pick<MangaRegionView, "bounds" | "vertical">>>
  >({});
  const [applying, setApplying] = useState(false);
  const rebuilding = activePageRebuild(jobs, page?.id);
  const pageBlocked = applying || !!rebuilding;
  const [previousRender, setPreviousRender] = useState<{
    pageId: string;
    assetId: string | null;
  } | null>(null);
  useEffect(() => {
    setEdits({});
  }, [page?.id]);
  const editedView = view
    ? { ...view, regions: view.regions.map((r) => ({ ...r, ...edits[r.id] })) }
    : null;
  const changeRegion = (
    id: string,
    patch: Partial<Pick<MangaRegionView, "bounds" | "vertical">>,
  ) => setEdits((old) => ({ ...old, [id]: { ...old[id], ...patch } }));
  async function applyRegions() {
    if (!view || pageBlocked) return;
    setPreviousRender({ pageId: view.page.id, assetId: view.renderedAssetId });
    setApplying(true);
    setError(null);
    try {
      const jobs = await projectApi.jobs({
        projectId,
        cursor: null,
        limit: 30,
      });
      if (
        jobs.some((job) =>
          ["queued", "running", "cancelling"].includes(job.state),
        )
      )
        throw new Error(t("mangaPageBusy"));
      let current = view;
      for (const [id, patch] of Object.entries(edits)) {
        for (const change of [
          patch.bounds
            ? { kind: "bounds" as const, bounds: patch.bounds }
            : null,
          patch.vertical !== undefined
            ? { kind: "direction" as const, vertical: patch.vertical }
            : null,
        ]) {
          if (!change) continue;
          const region = current.regions.find((r) => r.id === id);
          if (!region) throw new Error(t("conflict"));
          current = await projectApi.updateMangaRegion({
            projectId,
            regionId: id,
            patch: change,
            expectedRevision: region.revision,
          });
        }
      }
      setEdits({});
      const job = await projectApi.rebuildMangaPage({
        projectId,
        pageId: view.page.id,
      });
      await onJob(job);
      setShowTranslation(true);
    } catch (e) {
      setError(e);
    } finally {
      refresh();
      setApplying(false);
    }
  }
  const [showRegions, setShowRegions] = useState(false);
  const [selectedRegion, setSelectedRegion] = useState<string | null>(null);
  const activeRegion = view?.regions.some((r) => r.id === selectedRegion)
    ? selectedRegion
    : (view?.regions[0]?.id ?? null);
  if (batch)
    return (
      <div className="bc-manga-batch">
        <p className="bc-hint">
          {t("mangaBatchHint")} {t("page")} {selected + 1} / {pages.length}
        </p>
        <ProcessingStatus
          batch
          projectId={projectId}
          pageIds={loading ? [] : pages.slice(selected).map((p) => p.id)}
          onSettings={onSettings}
          setupVersion={setupVersion}
          t={t}
        />
      </div>
    );
  return (
    <div className="bc-manga">
      <aside>
        <select
          disabled={applying || Object.keys(edits).length > 0}
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
              disabled={applying || Object.keys(edits).length > 0}
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
        <ProcessingStatus
          onSettings={onSettings}
          setupVersion={setupVersion}
          projectId={projectId}
          pageIds={loading ? [] : pages.slice(selected).map((p) => p.id)}
          t={t}
        />
        {(error != null || pageError != null) && (
          <p role="alert" className="bc-error">
            {errorText(error ?? pageError, t)}
          </p>
        )}
        <div className="bc-toolbar">
          <button
            disabled={
              applying ||
              Object.keys(edits).length > 0 ||
              selected === 0 ||
              !page
            }
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
              disabled={applying || Object.keys(edits).length > 0}
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
            disabled={
              applying ||
              Object.keys(edits).length > 0 ||
              selected >= pages.length - 1
            }
            onClick={() =>
              setSelected((i) => Math.min(pages.length - 1, i + 1))
            }
            aria-label={t("nextPage")}
          >
            →
          </button>
          <label>
            {t("zoom")}{" "}
            <select
              disabled={pageBlocked}
              value={zoom}
              onChange={(e) => setZoom(e.target.value)}
            >
              <option value="fit">{t("fitPage")}</option>
              <option value="page">{t("wholePage")}</option>
              {[25, 50, 75, 100, 150, 200, 300].map((n) => (
                <option key={n} value={n}>
                  {n}%
                </option>
              ))}
            </select>
          </label>
          {view?.renderedAssetId && (
            <button
              disabled={pageBlocked}
              aria-pressed={showTranslation}
              onClick={() => setShowTranslation((value) => !value)}
            >
              {t(showTranslation ? "translation" : "original")}
            </button>
          )}
          <button
            disabled={pageBlocked}
            aria-pressed={showRegions}
            onClick={() => setShowRegions((value) => !value)}
          >
            {t("regions")}
          </button>
        </div>
        <div className="bc-manga-canvas-shell" aria-busy={pageBlocked}>
          <PageCanvas
            projectId={projectId}
            page={page}
            renderedAssetId={
              showTranslation
                ? (view?.renderedAssetId ??
                  (pageBlocked && previousRender?.pageId === page?.id
                    ? previousRender?.assetId
                    : null))
                : null
            }
            zoom={zoom}
            regions={showRegions ? editedView?.regions : undefined}
            onChangeBounds={
              pageBlocked
                ? undefined
                : (id, bounds) => changeRegion(id, { bounds })
            }
            selectedRegion={activeRegion}
            onSelectRegion={setSelectedRegion}
            blocked={pageBlocked}
            loading={loading}
            rtl={
              volumes.find((v) => v.id === page?.volumeId)?.readingDirection ===
              "rtl"
            }
            onNavigate={(delta) =>
              !applying &&
              !Object.keys(edits).length &&
              setSelected((i) =>
                Math.max(0, Math.min(pages.length - 1, i + delta)),
              )
            }
            t={t}
          />
          {pageBlocked && (
            <div
              className="bc-manga-page-loading"
              role="status"
              aria-live="polite"
            >
              <div>
                <span className="bc-loading-spinner" aria-hidden="true" />
                <strong>{t("mangaRebuildingPage")}</strong>
                <span>
                  {t(
                    rebuilding?.currentStage === "masks"
                      ? "mangaMasks"
                      : rebuilding?.currentStage === "inpainting"
                        ? "mangaInpainting"
                        : rebuilding?.currentStage === "lettering"
                          ? "mangaLettering"
                          : "processing",
                  )}
                </span>
                <progress
                  aria-label={t("mangaRebuildingPage")}
                  max={rebuilding?.totalSteps || undefined}
                  value={
                    rebuilding?.totalSteps
                      ? rebuilding.completedSteps
                      : undefined
                  }
                />
              </div>
            </div>
          )}
        </div>
      </div>
      {showRegions && (
        <RegionInspector
          view={editedView}
          busy={pageBlocked}
          changed={Object.keys(edits).length > 0}
          onDiscard={() => setEdits({})}
          onChangeDirection={(id, vertical) => changeRegion(id, { vertical })}
          onApply={() => void applyRegions()}
          selected={activeRegion}
          onSelect={setSelectedRegion}
          onClose={() => setShowRegions(false)}
          t={t}
        />
      )}
    </div>
  );
}
