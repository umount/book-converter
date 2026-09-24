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
        <strong>
          {t("jobs")}
          {jobs[0]
            ? ` · ${catalog.find((p) => p.descriptor.id === jobs[0].job.projectId)?.descriptor.name ?? ""}`
            : ""}
        </strong>
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
          <div className="bc-job-heading">
            <strong>
              {t(
                job.kind === "book_translation" ||
                  job.kind === "book_metadata" ||
                  job.kind === "book_glossary" ||
                  job.kind === "book_title" ||
                  job.kind === "book_retarget"
                  ? job.kind
                  : "processing",
              )}
            </strong>
            {job.currentChapterNumber != null && (
              <span className="bc-job-chapter">
                {t("jobChapter")} {job.currentChapterNumber}:{" "}
                {job.currentChapterTitle}
              </span>
            )}
            {["running", "queued"].includes(job.state) && job.currentStage && (
              <span className="bc-hint">
                {t(
                  job.currentStage === "context"
                    ? "jobContext"
                    : job.currentStage === "glossary"
                      ? "book_glossary"
                      : job.currentStage === "title"
                        ? "book_title"
                        : job.currentStage === "metadata"
                          ? "book_metadata"
                          : job.currentStage === "retarget"
                            ? "book_retarget"
                            : "translation",
                )}
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
          </div>
          <div className="bc-job-progress">
            <progress
              aria-label={t("processing")}
              value={job.completedChapters ?? job.completedSteps}
              max={Math.max(1, job.totalChapters ?? job.totalSteps)}
            />
            <span>
              {t(job.state)} ·{" "}
              {Math.round(
                ((job.completedChapters ?? job.completedSteps) /
                  Math.max(1, job.totalChapters ?? job.totalSteps)) *
                  100,
              )}
              % ·{" "}
              {job.totalChapters != null
                ? `${t("chapters")}: ${job.completedChapters}/${job.totalChapters}`
                : `${t("jobStages")}: ${job.completedSteps}/${job.totalSteps}`}
            </span>
            {["running", "queued"].includes(job.state) && (
              <span className="bc-hint">
                {job.remainingSeconds == null
                  ? t("estimatingTime")
                  : `${t("remainingTime")}: ${remainingTime(job.remainingSeconds, t)}`}
              </span>
            )}
          </div>
          {job.error && (
            <span className="bc-error">{errorText(job.error, t)}</span>
          )}
        </div>
      ))}
    </section>
  );
}
