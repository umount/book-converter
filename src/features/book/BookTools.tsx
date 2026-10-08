import { ReferenceMappings } from "./ReferenceMappings";
import { BookOverview } from "./BookOverview";
import { BookNarration } from "./BookNarration";
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import type {
  BookReferenceView,
  ChapterSummary,
  ProjectDescriptor,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
import type { BookEditorSession } from "../../shared/state/editor";
export type BookTool = "overview" | "reference" | "narration";
export function BookTools(props: Parameters<typeof Tools>[0]) {
  if (props.tool === "narration") return <BookNarration {...props} />;
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
  updateReferenceGlossary,
}: {
  updateReferenceGlossary: (ids: string[]) => Promise<void>;
  metadataRevision: string;
  translationControls: import("react").ReactNode;
  tool: BookTool;
  project: ProjectDescriptor;
  chapters: ChapterSummary[];
  session: BookEditorSession | null;
  t: T;
  run: (kind: "metadata" | "summary" | "glossary") => Promise<void>;
  refresh: () => Promise<void>;
  registerFlush: (flush: (() => Promise<void>) | null) => void;
}) {
  const savedMappings = useRef<BookReferenceView["mappings"]>([]);
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
            if (alive) {
              setReference(v);
              savedMappings.current = v.mappings;
            }
          })
        : Promise.resolve();
    void request.catch((e) => {
      if (alive) setError(e);
    });
    return () => {
      alive = false;
    };
  }, [project.id, tool]);
  async function acceptReference(value: BookReferenceView) {
    const previous = new Map(savedMappings.current.map(m => [m.chapterId, m.referenceId]));
    const changed = value.mappings.filter(m => previous.get(m.chapterId) !== m.referenceId).map(m => m.chapterId);
    setReference(value);
    setMappingDirty(false);
    await refresh();
    await updateReferenceGlossary(changed);
    savedMappings.current = value.mappings;
  }
  async function flushTools() {
    if (busy) throw new Error(t("processing"));
    if (mappingDirty && reference) {
      await acceptReference(
        await projectApi.mapReference({
          projectId: project.id,
          expectedFingerprint: reference.fingerprint,
          mappings: reference.mappings,
        }),
      );
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
                  // Import replaces the mappings; do not extract terms from the
                  // old draft immediately before replacing that reference.
                  await acceptReference(
                    await projectApi.importReference({
                      projectId: project.id,
                      path,
                    }),
                  );
                }
              })
            }
          >
            {t("referenceImport")}
          </button>
          <p className="bc-hint bc-reference-hint">{t("referenceGlossaryHint")}</p>
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
                    await acceptReference(
                      await projectApi.mapReference({
                        projectId: project.id,
                        expectedFingerprint: reference.fingerprint,
                        mappings: reference.mappings,
                      }),
                    );
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
