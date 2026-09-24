import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import { assetUrl } from "../../shared/api/assets";
import type {
  BookMetadataView,
  BookPresentation,
  ProjectDescriptor,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";

export function BookOverview({
  project,
  t,
  run,
  registerFlush,
}: {
  project: ProjectDescriptor;
  t: T;
  run: (kind: "metadata" | "glossary") => Promise<void>;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
}) {
  const [details, setDetails] = useState<BookPresentation | null>(null);
  const [metadata, setMetadata] = useState<BookMetadataView | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const lock = useRef(false);
  useEffect(() => {
    let alive = true;
    void Promise.all([
      projectApi.presentation({ projectId: project.id }),
      projectApi.metadata({ projectId: project.id }),
    ])
      .then(([d, m]) => {
        if (alive) {
          setDetails(d);
          setMetadata(m);
        }
      })
      .catch((e) => {
        if (alive) setError(e);
      });
    return () => {
      alive = false;
    };
  }, [project.id]);
  async function flush() {
    if (lock.current) throw new Error(t("processing"));
    if (!dirty || !details) return;
    lock.current = true;
    setBusy(true);
    try {
      const saved = await projectApi.updatePresentation({
        projectId: project.id,
        title: details.title,
        author: details.author,
        summary: details.summary,
        instructions: details.instructions,
        expectedRevision: details.revision,
      });
      setDetails(saved);
      setDirty(false);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  useEffect(() => {
    registerFlush(flush);
    return () => registerFlush(null);
  });
  async function act(work: () => Promise<void>) {
    if (lock.current) return;
    setError(null);
    try {
      await flush();
      lock.current = true;
      setBusy(true);
      await work();
    } catch (e) {
      setError(e);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  function change(
    field: "title" | "author" | "summary" | "instructions",
    value: string | null,
  ) {
    if (details) {
      setDetails({ ...details, [field]: value });
      setDirty(true);
    }
  }
  async function setCover(path: string | null) {
    // flush may have advanced the details revision before the file dialog opened.
    const current = await projectApi.presentation({ projectId: project.id });
    setDetails(
      await projectApi.setCover({
        projectId: project.id,
        path,
        expectedRevision: current.revision,
      }),
    );
  }
  return (
    <section className="bc-tool">
      <h2>{t("metadata")}</h2>
      <p className="bc-hint">{project.source.displayName}</p>
      {error != null && (
        <p className="bc-error" role="alert">
          {errorText(error, t)}
        </p>
      )}
      {!details ? (
        <p>{t("loading")}</p>
      ) : (
        <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, display: "grid", gap: 16 }}>
          {details.coverAssetId && (
            <img
              src={assetUrl(project.id, details.coverAssetId)}
              alt={t("cover")}
              style={{
                maxWidth: 180,
                maxHeight: 260,
                objectFit: "contain",
                marginBottom: 16,
              }}
            />
          )}
          <div className="bc-actions">
            <button
              onClick={() =>
                void act(async () => {
                  const path = await open({
                    multiple: false,
                    filters: [
                      {
                        name: t("cover"),
                        extensions: ["png", "jpg", "jpeg", "webp", "gif"],
                      },
                    ],
                  });
                  if (typeof path === "string") await setCover(path);
                })
              }
            >
              {t("chooseCover")}
            </button>
            {details.coverAssetId && (
              <button onClick={() => void act(() => setCover(null))}>
                {t("removeCover")}
              </button>
            )}
          </div>
          {(["title", "author", "summary"] as const).map((field) => (
            <label key={field}>
              {t(field === "title" ? "bookTitle" : field)}
              {field === "summary" ? (
                <textarea
                  rows={5}
                  value={details[field] ?? metadata?.[field] ?? ""}
                  onChange={(e) => change(field, e.target.value)}
                />
              ) : (
                <input
                  value={details[field] ?? metadata?.[field] ?? ""}
                  onChange={(e) => change(field, e.target.value)}
                />
              )}
              {details[field] !== null && (
                <button onClick={() => change(field, null)}>
                  {t("useGenerated")}
                </button>
              )}
            </label>
          ))}
          {metadata && !metadata.current && (
            <p className="bc-warning">{t("staleMetadata")}</p>
          )}
          <label>
            {t("bookPrompt")}
            <textarea
              rows={7}
              value={details.instructions}
              onChange={(e) => change("instructions", e.target.value)}
            />
          </label>
          <p className="bc-hint">{t("bookPromptHint")}</p>
          <div className="bc-actions">
            <button
              className="primary"
              disabled={!dirty}
              onClick={() => void act(async () => {})}
            >
              {t("save")}
            </button>
            <button onClick={() => void act(() => run("metadata"))}>
              {t("generateMetadata")}
            </button>
            <button
              onClick={() =>
                void act(async () =>
                  setMetadata(
                    await projectApi.metadata({ projectId: project.id }),
                  ),
                )
              }
            >
              {t("refresh")}
            </button>
          </div>
        </fieldset>
      )}
    </section>
  );
}
