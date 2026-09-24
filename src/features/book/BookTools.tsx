import { ReferenceMappings } from "./ReferenceMappings";
import { BookOverview } from "./BookOverview";
import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import type {
  BookExportFormat,
  BookReferenceView,
  BookReplacePreview,
  ChapterSummary,
  EntitySelection,
  IncompletePolicy,
  ProjectDescriptor,
} from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
import type { BookEditorSession } from "../../shared/state/editor";
export type BookTool =
  | "overview"
  | "reference"
  | "replace"
  | "export"
  | "instructions";
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
  const [search, setSearch] = useState(""),
    [replacement, setReplacement] = useState(""),
    [matchCase, setMatchCase] = useState(false),
    [scope, setScope] = useState("all");
  const [preview, setPreview] = useState<BookReplacePreview | null>(null),
    [instructions, setInstructions] = useState(
      session?.snapshot().view.instructions ?? "",
    );
  const [format, setFormat] = useState<BookExportFormat>("epub"),
    [policy, setPolicy] = useState<IncompletePolicy>("reject");
  const [instructionsDirty, setInstructionsDirty] = useState(false),
    [mappingDirty, setMappingDirty] = useState(false);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null),
    [notice, setNotice] = useState("");
  const selection: EntitySelection =
    scope === "chapter" && session
      ? { kind: "explicit_ids", ids: [session.snapshot().view.chapter.id] }
      : { kind: "all" };
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
    if (instructionsDirty && session) {
      const view = session.snapshot().view;
      await projectApi.updateInstructions({
        projectId: project.id,
        chapterId: view.chapter.id,
        instructions,
        expectedRevision: view.chapter.revision,
      });
      setInstructionsDirty(false);
      await session.refresh();
    }
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
  const scopeSelect = (
    <label>
      {t("selection")}
      <select
        disabled={busy}
        value={scope}
        onChange={(e) => {
          setScope(e.target.value);
          setPreview(null);
        }}
      >
        <option value="all">{t("allChapters")}</option>
        <option value="chapter" disabled={!session}>
          {t("selectedChapter")}
        </option>
      </select>
    </label>
  );
  return (
    <div className="bc-tool">
      <h2>{t(tool === "overview" ? "metadata" : tool)}</h2>
      {tool === "instructions" && (
        <>
          <p className="bc-hint">{t("instructionHint")}</p>
          <textarea
            aria-label={t("instructions")}
            rows={10}
            value={instructions}
            disabled={busy || !session}
            onChange={(e) => {
              setInstructions(e.target.value);
              setInstructionsDirty(true);
            }}
          />
          <button
            className="primary"
            disabled={busy || !session}
            onClick={() =>
              void act(async () => {
                const view = session!.snapshot().view;
                await projectApi.updateInstructions({
                  projectId: project.id,
                  chapterId: view.chapter.id,
                  instructions,
                  expectedRevision: view.chapter.revision,
                });
                setInstructionsDirty(false);
                await session!.refresh();
                setNotice(t("saved"));
              })
            }
          >
            {t("save")}
          </button>
        </>
      )}
      {tool === "replace" && (
        <>
          <div className="bc-fields">
            <label>
              {t("find")}
              <input
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                  setPreview(null);
                }}
              />
            </label>
            <label>
              {t("replacement")}
              <input
                value={replacement}
                onChange={(e) => {
                  setReplacement(e.target.value);
                  setPreview(null);
                }}
              />
            </label>
            {scopeSelect}
          </div>
          <label className="bc-check">
            <input
              type="checkbox"
              checked={matchCase}
              onChange={(e) => {
                setMatchCase(e.target.checked);
                setPreview(null);
              }}
            />
            {t("caseSensitive")}
          </label>
          <button
            disabled={busy || !search}
            onClick={() =>
              void act(async () =>
                setPreview(
                  await projectApi.previewReplace({
                    projectId: project.id,
                    selection,
                    search,
                    replacement,
                    caseSensitive: matchCase,
                  }),
                ),
              )
            }
          >
            {t("previewChanges")}
          </button>
          {preview && (
            <>
              <p>
                {t("changes")}: {preview.changes.length}
              </p>
              <div className="bc-change-list">
                {preview.changes.map((change) => (
                  <article key={change.blockId}>
                    <strong>
                      {chapters.find((c) => c.id === change.chapterId)?.title}
                    </strong>
                    <del>{change.before}</del>
                    <ins>{change.after}</ins>
                  </article>
                ))}
              </div>
              <button
                className="primary"
                disabled={busy || !preview.changes.length}
                onClick={() =>
                  void act(async () => {
                    await projectApi.applyReplace({
                      projectId: project.id,
                      previewId: preview.previewId,
                    });
                    setPreview(null);
                    await refresh();
                    setNotice(t("saved"));
                  })
                }
              >
                {t("apply")}
              </button>
            </>
          )}
        </>
      )}
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
      {tool === "export" && (
        <>
          <div className="bc-fields">
            <label>
              {t("format")}
              <select
                value={format}
                onChange={(e) => setFormat(e.target.value as BookExportFormat)}
              >
                {["epub", "fb2", "txt", "pdf"].map((v) => (
                  <option key={v} value={v}>
                    {v.toUpperCase()}
                  </option>
                ))}
              </select>
            </label>
            {scopeSelect}
            <label>
              {t("unfinished")}
              <select
                value={policy}
                onChange={(e) => setPolicy(e.target.value as IncompletePolicy)}
              >
                <option value="reject">{t("reject")}</option>
                <option value="originals">{t("originals")}</option>
              </select>
            </label>
          </div>
          <button
            className="primary"
            disabled={busy}
            onClick={() =>
              void act(async () => {
                const destination = await save({
                  defaultPath: `${project.name}.${format}`,
                  filters: [
                    { name: format.toUpperCase(), extensions: [format] },
                  ],
                });
                if (destination) {
                  await projectApi.exportBook({
                    projectId: project.id,
                    selection,
                    format,
                    incompletePolicy: policy,
                    destination,
                  });
                  setNotice(t("exported"));
                }
              })
            }
          >
            {t("chooseDestination")}
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void act(async () => {
                const destination = await save({
                  defaultPath: `${project.name}.bcproj`,
                  filters: [
                    { name: t("exportArchive"), extensions: ["bcproj"] },
                  ],
                });
                if (destination) {
                  await projectApi.exportArchive({
                    projectId: project.id,
                    destination,
                  });
                  setNotice(t("exported"));
                }
              })
            }
          >
            {t("exportArchive")}
          </button>
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
