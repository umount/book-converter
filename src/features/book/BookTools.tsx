import { ReferenceMappings } from "./ReferenceMappings";
import { BookOverview } from "./BookOverview";
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import type {
  BookReferenceView,
  ChapterSummary,
  ProjectDescriptor,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
import type { BookEditorSession } from "../../shared/state/editor";
export type BookTool = "overview" | "reference";
export function BookTools(props: Parameters<typeof Tools>[0]) {
  return props.tool === "overview" ? (
    <BookOverview key={props.project.id} {...props} />
  ) : (
    <Tools {...props} />
  );
}
function Tools({
  tool,
  project,
  chapters,
  session,
  t,
  refresh,
  registerFlush,
}: {
  translationControls: import("react").ReactNode;
  tool: BookTool;
  project: ProjectDescriptor;
  chapters: ChapterSummary[];
  session: BookEditorSession | null;
  t: T;
  run: (kind: "metadata" | "glossary") => Promise<void>;
  refresh: () => Promise<void>;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
}) {
  const [reference, setReference] = useState<BookReferenceView | null>(null);
  const [mappingDirty, setMappingDirty] = useState(false);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null),
    [notice, setNotice] = useState("");
  async function act(work: () => Promise<void>) {
    setError(null);
    setNotice("");
    setBusy(true);
    try {
      await session?.flush();
      await work();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    let alive = true;
    setReference(null);
    const request =
      tool === "reference"
        ? projectApi.reference({ projectId: project.id }).then((v) => {
            if (alive) setReference(v);
          })
        : Promise.resolve();
    void request.catch((e) => {
      if (alive) setError(e);
    });
    return () => {
      alive = false;
    };
  }, [project.id, tool]);
  async function flushTools() {
    if (busy) throw new Error(t("processing"));
    if (mappingDirty && reference) {
      setReference(
        await projectApi.mapReference({
          projectId: project.id,
          expectedFingerprint: reference.fingerprint,
          mappings: reference.mappings,
        }),
      );
      setMappingDirty(false);
      await refresh();
    }
  }
  useEffect(() => {
    registerFlush(flushTools);
    return () => registerFlush(null);
  });
  return (
    <div className="bc-tool">
      <h2>{t(tool === "overview" ? "metadata" : tool)}</h2>
      {tool === "reference" && (
        <>
          <button
            disabled={busy}
            onClick={() =>
              void act(async () => {
                const path = await open({
                  multiple: false,
                  filters: [
                    {
                      name: t("reference"),
                      extensions: ["txt", "fb2", "epub", "pdf", "zip"],
                    },
                  ],
                });
                if (typeof path === "string") {
                  await flushTools();
                  setReference(
                    await projectApi.importReference({
                      projectId: project.id,
                      path,
                    }),
                  );
                  await refresh();
                }
              })
            }
          >
            {t("referenceImport")}
          </button>
          {!reference?.chapters.length ? (
            <p className="bc-hint">{t("noReference")}</p>
          ) : (
            <>
              <ReferenceMappings
                chapters={chapters}
                reference={reference}
                disabled={busy}
                t={t}
                onChange={(value) => {
                  setReference(value);
                  setMappingDirty(true);
                }}
              />
              <button
                className="primary"
                disabled={busy}
                onClick={() =>
                  void act(async () => {
                    setReference(
                      await projectApi.mapReference({
                        projectId: project.id,
                        expectedFingerprint: reference.fingerprint,
                        mappings: reference.mappings,
                      }),
                    );
                    setMappingDirty(false);
                    await refresh();
                    setNotice(t("saved"));
                  })
                }
              >
                {t("saveMappings")}
              </button>
            </>
          )}
        </>
      )}
      {busy && <p role="status">{t("loading")}</p>}
      {notice && (
        <p role="status" className="bc-success">
          {notice}
        </p>
      )}
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
    </div>
  );
}
