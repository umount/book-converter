import { useEffect, useState } from "react";
import { projectApi } from "../../shared/api/projects";
import type {
  MangaPreflight,
  MangaStage,
  JobView,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";

const stages: Record<MangaStage, Parameters<T>[0]> = {
  detection: "mangaDetection",
  recognition: "mangaRecognition",
  translation: "mangaTranslation",
  masks: "mangaMasks",
  inpainting: "mangaInpainting",
  lettering: "mangaLettering",
};
const reasons: Record<string, Parameters<T>[0]> = {
  mangaRuntimeMissing: "mangaRuntimeMissing",
  mangaMaskModelRequired: "mangaMaskModelRequired",
  mangaCleanupModelRequired: "mangaCleanupModelRequired",
  mangaRecognitionProfileRequired: "mangaRecognitionProfileRequired",
  mangaTranslationProfileRequired: "mangaTranslationProfileRequired",
  mangaProfileKeyRequired: "mangaProfileKeyRequired",
  mangaProfileInvalid: "mangaProfileInvalid",
  mangaTranslationUnavailable: "mangaTranslationUnavailable",
  mangaMasksUnavailable: "mangaMasksUnavailable",
  mangaInpaintingUnavailable: "mangaInpaintingUnavailable",
  mangaLetteringUnavailable: "mangaLetteringUnavailable",
};

/** Opening this panel only checks local configuration. It never starts processing. */
export function ProcessingStatus({
  projectId,
  pageIds,
  t,
}: {
  projectId: string;
  pageIds: string[];
  t: T;
}) {
  const [count, setCount] = useState(10);
  const [starting, setStarting] = useState(false);
  const [activeJob, setActiveJob] = useState<JobView | null>(null);
  const [open, setOpen] = useState(false);
  const [version, setVersion] = useState(0);
  const [result, setResult] = useState<MangaPreflight | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let active = true;
    setResult(null);
    setError(null);
    void projectApi
      .mangaPreflight({ projectId })
      .then((value) => {
        if (active) setResult(value);
      })
      .catch((reason) => {
        if (active) setError(reason);
      });
    return () => {
      active = false;
    };
  }, [projectId, version]);
  useEffect(() => {
    let alive = true;
    let generation = 0;
    const refresh = async () => {
      const request = ++generation;
      try {
        const jobs = await projectApi.jobs({
          projectId,
          cursor: null,
          limit: 30,
        });
        if (alive && request === generation)
          setActiveJob(
            jobs.find((job) =>
              ["queued", "running", "cancelling"].includes(job.state),
            ) ?? null,
          );
      } catch (reason) {
        if (alive) setError(reason);
      }
    };
    void refresh();
    const subscription = projectApi.subscribe(projectId, (event) => {
      if (event.type === "job.updated") void refresh();
    });
    void subscription.ready.catch((reason) => {
      if (alive) setError(reason);
    });
    return () => {
      alive = false;
      subscription.dispose();
    };
  }, [projectId, version]);
  const ready = result?.requirements.every((item) => item.available) ?? false;
  async function start() {
    setStarting(true);
    setError(null);
    try {
      await projectApi.startMangaAutomatic({
        projectId,
        selection: { kind: "explicit_ids", ids: pageIds },
        options: { maxPages: count, force: false },
      });
      setVersion((value) => value + 1);
    } catch (reason) {
      setError(reason);
    } finally {
      setStarting(false);
    }
  }
  return (
    <section className="bc-manga-processing">
      <div className="bc-toolbar">
        <label>
          {t("pages")}{" "}
          <input
            type="number"
            min={1}
            max={pageIds.length || 1}
            value={count}
            onChange={(event) => setCount(Number(event.target.value))}
            style={{ width: 64 }}
          />
        </label>
        <button
          disabled={
            !ready ||
            starting ||
            !!activeJob ||
            !pageIds.length ||
            !Number.isInteger(count) ||
            count < 1
          }
          title={t("mangaBatchHint")}
          onClick={() => void start()}
        >
          {t(starting ? "processing" : "translateBatch")}
        </button>
        {activeJob && (
          <span role="status" className="bc-hint">
            {t("pages")}: {activeJob.completedPages ?? 0}/
            {activeJob.totalPages ?? 0}
          </span>
        )}
      </div>
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
      <details
        className="bc-manga-preflight"
        onToggle={(event) => setOpen(event.currentTarget.open)}
      >
        <summary>
          {t("mangaPreflight")}
          {ready ? ` · ${t("mangaConfigured")}` : ""}
        </summary>
        {open && (
          <>
            <p className="bc-hint">{t("mangaPreflightHint")}</p>
            {error != null ? (
              <p role="alert" className="bc-error">
                {errorText(error, t)}
              </p>
            ) : !result ? (
              <p role="status">{t("loading")}</p>
            ) : (
              <ul>
                {result.requirements.map((item) => (
                  <li key={item.stage}>
                    <strong>{t(stages[item.stage])}</strong>
                    <span>
                      {t(
                        item.available
                          ? "mangaConfigured"
                          : (reasons[item.reasonKey ?? ""] ??
                              "mangaCapabilityUnavailable"),
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <button onClick={() => setVersion((value) => value + 1)}>
              {t("refresh")}
            </button>
          </>
        )}
      </details>
    </section>
  );
}
