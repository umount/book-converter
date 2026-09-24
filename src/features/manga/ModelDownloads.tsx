import { useEffect, useState } from "react";
import { modelApi } from "../../shared/api/models";
import type { ModelFailure, ModelView } from "../../shared/contracts/generated";
import type { T } from "../../app/strings";
const active = (view: ModelView) =>
  view.status === "downloading" || view.status === "verifying";
const megabytes = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
function failureText(error: unknown, t: T) {
  const keys: Record<ModelFailure, Parameters<T>[0]> = {
    unknown_model: "modelUnknown",
    busy: "modelBusy",
    network: "modelNetwork",
    size_mismatch: "modelSizeError",
    checksum_mismatch: "modelHashError",
    storage: "modelStorageError",
    unsafe_path: "modelStorageError",
  };
  return t(keys[error as ModelFailure] ?? "error");
}
export function ModelDownloads({ t }: { t: T }) {
  const [models, setModels] = useState<ModelView[] | null>(null),
    [error, setError] = useState<unknown>(null),
    [busy, setBusy] = useState(false);
  const downloading = models?.some(active) ?? false;
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function refresh() {
      try {
        const next = await modelApi.list();
        if (alive) {
          setModels(next);
          if (next.some(active)) timer = setTimeout(() => void refresh(), 500);
        }
      } catch (e) {
        if (alive) setError(e);
      }
    }
    void refresh();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [downloading]);
  async function act(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setModels(await modelApi.list());
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="bc-models" aria-label={t("mangaModels")}>
      <h3>{t("mangaModels")}</h3>
      <p className="bc-hint">{t("modelDownloadHint")}</p>
      {!models && <p role="status">{t("loading")}</p>}
      {models?.map((view) => (
        <article className="bc-model" key={view.model.id}>
          <header>
            <strong>{view.model.name}</strong>
            <span>{megabytes(view.model.bytes)}</span>
          </header>
          <p className="bc-hint">
            Hugging Face · {view.model.repository} · {view.model.license}
          </p>
          {view.model.experimental && (
            <p className="bc-warning">{t("modelExperimental")}</p>
          )}
          <div role="status">
            <span>
              {t(
                (
                  {
                    missing: "modelMissing",
                    downloading: "modelDownloading",
                    verifying: "modelVerifying",
                    paused: "modelPaused",
                    downloaded: "modelDownloaded",
                    failed: "failed",
                  } as const
                )[view.status],
              )}
            </span>
            {view.status !== "missing" && (
              <span>
                {" "}
                · {megabytes(view.downloadedBytes)} /{" "}
                {megabytes(view.model.bytes)}
              </span>
            )}
          </div>
          {active(view) && (
            <progress
              aria-label={t("modelDownloading")}
              value={view.downloadedBytes}
              max={view.model.bytes}
            />
          )}
          {view.failure && (
            <p role="alert" className="bc-error">
              {failureText(view.failure, t)}
            </p>
          )}
          <div className="bc-model-actions">
            {active(view) ? (
              <button
                disabled={busy}
                onClick={() => void act(() => modelApi.pause(view.model.id))}
              >
                {t("modelPause")}
              </button>
            ) : (
              <>
                {view.status !== "downloaded" && (
                  <button
                    className="primary"
                    disabled={busy || downloading}
                    onClick={() =>
                      void act(() => modelApi.download(view.model.id))
                    }
                  >
                    {t(view.downloadedBytes > 0 ? "resume" : "modelDownload")}
                  </button>
                )}
                {view.status !== "missing" && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      void act(() => modelApi.remove(view.model.id))
                    }
                  >
                    {t("modelRemove")}
                  </button>
                )}
              </>
            )}
          </div>
        </article>
      ))}
      {error != null && (
        <p role="alert" className="bc-error">
          {failureText(error, t)}{" "}
          <button disabled={busy} onClick={() => void act(async () => {})}>
            {t("retry")}
          </button>
        </p>
      )}
    </section>
  );
}
