import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import type { BookExportFormat, IncompletePolicy, ProjectDescriptor, } from "../../shared/contracts/generated";
import { Modal } from "../../shared/ui/Modal";
import { errorText, type T } from "../../app/strings";
export type ExportFormat = BookExportFormat | "bcproj";
export function ExportMenu({ choose, disabled, t, }: {
    project: ProjectDescriptor;
    choose: (format: ExportFormat) => void;
    disabled: boolean;
    t: T;
}) {
    return (<details className="bc-export-menu" onBlur={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null))
                e.currentTarget.open = false;
        }} onKeyDown={(e) => {
            if (e.key === "Escape") {
                e.currentTarget.open = false;
                e.currentTarget.querySelector("summary")?.focus();
            }
        }}>
      <summary>{t("export")}</summary>
      <div className="bc-export-options">
        {((["epub", "fb2", "txt", "pdf"] as const).map((format) => (<button key={format} disabled={disabled} onClick={(e) => {
                e.currentTarget.closest("details")!.open = false;
                choose(format);
            }}>
              {format === "fb2" ? "FB2 (ZIP)" : format.toUpperCase()}
            </button>)))}
        <button disabled={disabled} onClick={(e) => {
            e.currentTarget.closest("details")!.open = false;
            choose("bcproj");
        }}>
          {t("exportArchive")}
        </button>
      </div>
    </details>);
}
export function ProjectExport({ project, format, chapterId, beforeExport, close, t, }: {
    project: ProjectDescriptor;
    format: ExportFormat;
    chapterId: string | null;
    beforeExport: () => Promise<void>;
    close: () => void;
    t: T;
}) {
    const [scope, setScope] = useState("all");
    const [policy, setPolicy] = useState<IncompletePolicy>("translated_only");
    const [busy, setBusy] = useState(false), [error, setError] = useState<unknown>(null);
    async function run() {
        setBusy(true);
        setError(null);
        try {
            await beforeExport();
            const destination = await save({
                defaultPath: `${project.name}.${format === "fb2" ? "fb2.zip" : format}`,
                filters: [
                    {
                        name: format.toUpperCase(),
                        extensions: [format === "fb2" ? "zip" : format],
                    },
                ],
            });
            if (!destination)
                return;
            if (format === "bcproj")
                await projectApi.exportArchive({ projectId: project.id, destination });
            else
                await projectApi.exportBook({
                    projectId: project.id,
                    destination,
                    overwrite: true,
                    format,
                    incompletePolicy: policy,
                    selection: scope === "chapter" && chapterId
                        ? { kind: "explicit_ids", ids: [chapterId] }
                        : { kind: "all" },
                });
            close();
        }
        catch (e) {
            setError(e);
        }
        finally {
            setBusy(false);
        }
    }
    return (<Modal title={`${t("export")} · ${format === "fb2" ? "FB2 (ZIP)" : format.toUpperCase()}`} closeLabel={t("close")} onClose={close} busy={busy} footer={<button className="primary" disabled={busy} onClick={() => void run()}>
          {busy ? t("exporting") : t("chooseDestination")}
        </button>}>
      {format !== "bcproj" && (<div className="bc-fields">
          <label>
            {t("selection")}
            <select disabled={busy} value={scope} onChange={(e) => setScope(e.target.value)}>
              <option value="all">{t("allChapters")}</option>
              <option value="chapter" disabled={!chapterId}>
                {t("selectedChapter")}
              </option>
            </select>
          </label>
          <label>
            {t("unfinished")}
            <select disabled={busy} value={policy} onChange={(e) => setPolicy(e.target.value as IncompletePolicy)}>
              <option value="translated_only">{t("translatedOnly")}</option>
              <option value="originals">{t("originals")}</option>
            </select>
          </label>
          {policy === "translated_only" && <p>{t("translatedOnlyHint")}</p>}
        </div>)}
      {busy && (<div className="bc-export-progress" role="status">
          <span className="bc-loading-spinner" aria-hidden="true"/>
          <span>{t("exporting")}</span>
        </div>)}
      {error != null && (<p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>)}
    </Modal>);
}
