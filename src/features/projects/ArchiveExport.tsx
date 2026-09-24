import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { projectApi } from "../../shared/api/projects";
import type { ProjectDescriptor } from "../../shared/contracts/generated";
import { errorText, type T } from "../../app/strings";
export function ArchiveExport({
  project,
  t,
}: {
  project: ProjectDescriptor;
  t: T;
}) {
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<unknown>(null),
    [done, setDone] = useState(false);
  async function exportArchive() {
    setBusy(true);
    setError(null);
    setDone(false);
    try {
      const destination = await save({
        defaultPath: `${project.name}.bcproj`,
        filters: [{ name: t("exportArchive"), extensions: ["bcproj"] }],
      });
      if (destination) {
        await projectApi.exportArchive({ projectId: project.id, destination });
        setDone(true);
      }
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="bc-tool">
      <h2>{t("export")}</h2>
      <button disabled={busy} onClick={() => void exportArchive()}>
        {t("exportArchive")}
      </button>
      {done && <p role="status">{t("exported")}</p>}
      {error != null && (
        <p role="alert" className="bc-error">
          {errorText(error, t)}
        </p>
      )}
    </div>
  );
}
