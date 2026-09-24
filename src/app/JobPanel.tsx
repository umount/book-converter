import type {
  JobRef,
  JobView,
  ProjectSummary,
} from "../shared/contracts/generated";
import { errorText, type T } from "./strings";
function remainingTime(seconds: number, t: T): string {
  if (seconds < 60) return `${seconds} ${t("etaSeconds")}`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${minutes} ${t("etaMinutes")}`;
  return `${Math.floor(minutes / 60)} ${t("etaHours")} ${minutes % 60} ${t("etaMinutes")}`;
}
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
        <button
          className="bc-icon-button"
          aria-label={t("close")}
          title={t("close")}
          onClick={close}
        >
          ×
        </button>
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
                job.kind === "book_glossary" ||
                job.kind === "book_title"
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
            <span className="bc-hint">
              {job.remainingSeconds == null
                ? t("estimatingTime")
                : `${t("remainingTime")}: ${remainingTime(job.remainingSeconds, t)}`}
            </span>
          )}
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
