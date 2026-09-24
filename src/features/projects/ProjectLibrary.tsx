import type { ProjectSummary } from "../../shared/contracts/generated";
import type { T } from "../../app/strings";
export function ProjectLibrary({
  catalog,
  busy,
  t,
  create,
  importArchive,
  open,
  remove,
}: {
  catalog: ProjectSummary[];
  busy: boolean;
  t: T;
  create: () => void;
  importArchive: () => void;
  open: (id: string) => void;
  remove: (id: string) => void;
}) {
  return (
    <main className="bc-library">
      <p className="bc-eyebrow">BOOK / MANGA</p>
      <h1>{t("welcome")}</h1>
      <p className="bc-hint">{t("welcomeHint")}</p>
      <div className="bc-actions">
        <button className="primary" onClick={create}>
          {t("newProject")}
        </button>
        <button disabled={busy} onClick={importArchive}>
          {t("importArchive")}
        </button>
      </div>
      <div className="bc-project-grid">
        {catalog.map(({ descriptor: project, progress }) => (
          <article key={project.id}>
            <span className="bc-eyebrow">{t(project.kind)}</span>
            <h2>{project.name}</h2>
            <p className="bc-hint">{project.source.displayName}</p>
            <p className="bc-hint">
              {progress.kind === "book"
                ? `${t("chapters")}: ${progress.translated} / ${progress.chapters}`
                : `${t("pages")}: ${progress.approved} / ${progress.pages}`}
            </p>
            <footer>
              <button disabled={busy} onClick={() => open(project.id)}>
                {t("open")}
              </button>
              <button
                className="danger"
                disabled={busy}
                onClick={() => remove(project.id)}
              >
                {t("delete")}
              </button>
            </footer>
          </article>
        ))}
      </div>
      {!catalog.length && <p className="bc-hint">{t("emptyProjects")}</p>}
    </main>
  );
}
