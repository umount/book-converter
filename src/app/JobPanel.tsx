import type {
  JobRef,
  JobView,
  ProjectSummary,
} from "../shared/contracts/generated";
import { errorText, type T } from "./strings";
export function JobPanel({
  jobs,
  catalog,
  t,
  busy,
  close,
  cancel,
  resume,
}: {
  jobs: JobView[];
  catalog: ProjectSummary[];
  t: T;
  busy: boolean;
  close: () => void;
  cancel: (job: JobRef) => void;
  resume: (job: JobRef) => void;
}) {
  return (
    <section className="bc-jobs">
      <header>
        <strong>{t("jobs")}</strong>
        <button onClick={close}>{t("close")}</button>
      </header>
      {!jobs.length && <p className="bc-hint">{t("noJobs")}</p>}
      {jobs.map((job) => (
        <div className="bc-job" key={`${job.job.projectId}/${job.job.jobId}`}>
          <span>
            {
              catalog.find((p) => p.descriptor.id === job.job.projectId)
                ?.descriptor.name
            }
          </span>
          <strong>
            {t(
              job.kind === "book_translation" ||
                job.kind === "book_metadata" ||
                job.kind === "book_glossary"
                ? job.kind
                : "processing",
            )}
          </strong>
          <progress
            aria-label={t("processing")}
            value={job.completedSteps}
            max={Math.max(1, job.totalSteps)}
          />
          <span>
            {t(job.state)} · {job.completedSteps}/{job.totalSteps}
          </span>
          {["running", "queued"].includes(job.state) && (
            <button disabled={busy} onClick={() => cancel(job.job)}>
              {t("cancel")}
            </button>
          )}
          {["failed", "cancelled", "interrupted"].includes(job.state) && (
            <button disabled={busy} onClick={() => resume(job.job)}>
              {t("resume")}
            </button>
          )}
          {job.error && (
            <span className="bc-error">{errorText(job.error, t)}</span>
          )}
        </div>
      ))}
    </section>
  );
}
