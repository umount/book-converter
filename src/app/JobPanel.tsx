import { ToolbarIcon } from "../shared/ui/ToolbarIcon";
import { AudioPreviewPlayer } from "../shared/ui/AudioPreviewPlayer";
import type { AudioJobArgs, AudioJobView, JobRef, JobView, ProjectSummary, } from "../shared/contracts/generated";
import { errorText, type T } from "./strings";
function remainingTime(seconds: number, t: T): string {
    if (seconds < 60)
        return `${seconds} ${t("etaSeconds")}`;
    const minutes = Math.ceil(seconds / 60);
    if (minutes < 60)
        return `${minutes} ${t("etaMinutes")}`;
    return `${Math.floor(minutes / 60)} ${t("etaHours")} ${minutes % 60} ${t("etaMinutes")}`;
}
type PanelEntry = { type: "processing"; job: JobView } | { type: "audio"; job: AudioJobView };
export function JobPanel({ jobs, audioJobs, audioRunning, pauseAudio, resumeAudio, openNarration, catalog, t, busy, close, cancel, resume, clear, }: {
    jobs: JobView[];
    audioJobs: AudioJobView[];
    audioRunning: boolean;
    pauseAudio: (job: AudioJobArgs) => void;
    resumeAudio: (job: AudioJobArgs) => void;
    openNarration: () => void;
    catalog: ProjectSummary[];
    t: T;
    busy: boolean;
    close: () => void;
    cancel: (job: JobRef) => void;
    resume: (job: JobRef) => void;
    clear: () => void;
}) {
    const activeStates = ["running", "pausing", "cancelling", "queued"];
    const orderedJobs: PanelEntry[] = [
        ...jobs.map(job => ({ type: "processing" as const, job })),
        ...audioJobs.map(job => ({ type: "audio" as const, job })),
    ].sort((a, b) => {
        const rank = (state: string) => state === "queued" ? 1 : activeStates.includes(state) ? 0 : 2;
        return rank(a.job.state) - rank(b.job.state);
    });
    const current = orderedJobs.find(entry => activeStates.includes(entry.job.state));
    const projectId = jobs[0]?.job.projectId ?? audioJobs[0]?.projectId;
    return (<section className="bc-jobs">
      <header>
        <strong>
          {t("jobs")}
          {projectId
            ? ` · ${catalog.find((p) => p.descriptor.id === projectId)?.descriptor.name ?? ""}`
            : ""}
        </strong>
        <button className="bc-icon-button" aria-label={t("clearFinishedJobs")} title={t("clearFinishedJobs")} disabled={!orderedJobs.some(entry => !activeStates.includes(entry.job.state))} onClick={clear}>
          <ToolbarIcon name="clear"/>
        </button>
        <button className="bc-icon-button" aria-label={t("close")} title={t("close")} onClick={close}>
          ×
        </button>
      </header>
      {current && <progress className="bc-jobs-progress" aria-label={t(current.type === "audio" ? "audioProgress" : "processing")} value={current.type === "audio" ? current.job.completedChunks : current.job.completedSteps} max={Math.max(1, current.type === "audio" ? current.job.totalChunks : current.job.totalSteps)}/>}
      <div className="bc-jobs-body">
        {!orderedJobs.length && <p className="bc-hint">{t("noJobs")}</p>}
        {orderedJobs.map((entry) => {
          if (entry.type === "audio") return <AudioJobRow key={`audio/${entry.job.projectId}/${entry.job.id}`} job={entry.job} t={t} busy={busy} audioRunning={audioRunning} pause={pauseAudio} resume={resumeAudio} openNarration={openNarration}/>;
          const job = entry.job;
          return <div className={`bc-job${job.state === "succeeded" ? " bc-job-complete" : ""}`} key={`${job.job.projectId}/${job.job.jobId}`}>
            <div className="bc-job-heading">
              <strong>
                {t(job.kind === "book_translation" ||
                job.kind === "book_metadata" ||
                job.kind === "book_glossary" ||
                job.kind === "book_title" ||
                job.kind === "book_retarget"
                ? job.kind
                : "processing")}
              </strong>
              {job.currentChapterNumber != null && (<span className="bc-job-chapter">
                  {t("jobChapter")} {job.currentChapterNumber}:{" "}
                  {job.currentChapterTitle}
                </span>)}
              {["running", "queued"].includes(job.state) && job.currentStage && (<span className="bc-hint">
                  {t(job.currentStage === "context"
                                    ? "jobContext"
                                    : job.currentStage === "glossary"
                                        ? "book_glossary"
                                        : job.currentStage === "title"
                                            ? "book_title"
                                            : job.currentStage === "metadata"
                                                ? "book_metadata"
                                                : job.currentStage === "retarget"
                                                    ? "book_retarget"
                                                    : "translation")}
                </span>)}
              {["running", "queued"].includes(job.state) && (<button disabled={busy} onClick={() => cancel(job.job)}>
                  {t("cancel")}
                </button>)}
              {["failed", "cancelled", "interrupted"].includes(job.state) && (<button disabled={busy} onClick={() => resume(job.job)}>
                  {t("resume")}
                </button>)}
            </div>
            <div className="bc-job-progress">
              <span>
                {t(job.state)}{job.state !== "succeeded" && <> ·{" "}
                {Math.round((job.completedSteps / Math.max(1, job.totalSteps)) * 100)}
                % ·{" "}
                {job.totalChapters != null
                        ? `${t("chapters")}: ${job.completedChapters}/${job.totalChapters}`
                        : `${t("jobStages")}: ${job.completedSteps}/${job.totalSteps}`}
              </>}
              </span>
              {["running", "queued"].includes(job.state) && (<span className="bc-hint">
                  {job.remainingSeconds == null
                    ? t("estimatingTime")
                    : `${t("remainingTime")}: ${remainingTime(job.remainingSeconds, t)}`}
                </span>)}
            </div>
            {job.error && (<span className="bc-error">{errorText(job.error, t)}</span>)}
          </div>;
        })}
      </div>
    </section>);
}

function AudioJobRow({ job, t, busy, audioRunning, pause, resume, openNarration }: {
    job: AudioJobView;
    t: T;
    busy: boolean;
    audioRunning: boolean;
    pause: (job: AudioJobArgs) => void;
    resume: (job: AudioJobArgs) => void;
    openNarration: () => void;
}) {
    const args = { projectId: job.projectId, jobId: job.id };
    const state = { running: "audioRunning", pausing: "audioPausing", paused: "audioPaused", interrupted: "audioInterrupted", failed: "audioFailed", succeeded: "audioFinished" } as const;
    return <div className={`bc-job${job.state === "succeeded" ? " bc-job-complete" : ""}`}>
      <div className="bc-job-heading">
        <strong>{t("narration")} · {job.voice}</strong>
        {job.currentChapter && <span className="bc-job-chapter">{job.currentChapter}</span>}
        {job.state === "running" && !job.currentChapter && <span className="bc-hint">{t("audioPreparing")}</span>}
        <span className="bc-hint">{t(job.text === "original" ? "audioOriginal" : "audioTranslation")} · {job.language}</span>
        {job.state === "running" && <button disabled={busy} onClick={() => pause(args)}>{t("audioPause")}</button>}
        {job.state === "pausing" && <span className="bc-hint">{t("audioPauseHint")}</span>}
        {["paused", "failed", "interrupted"].includes(job.state) && <button disabled={busy || audioRunning} onClick={() => resume(args)}>{t("audioResume")}</button>}
        <button disabled={busy} onClick={openNarration}>{t("narration")}</button>
      </div>
      <div className="bc-job-progress">
        <span>{t(state[job.state])} · {Math.round(job.completedChunks / Math.max(1, job.totalChunks) * 100)}% · {t("chapters")}: {job.completedChapters}/{job.totalChapters}</span>
        <span className="bc-hint">{t("audioProgress")}: {job.completedChunks}/{job.totalChunks}</span>
      </div>
      {job.error && <span className="bc-error">{errorText(job.error, t)}</span>}
      <AudioPreviewPlayer job={job} t={t} />
    </div>;
}
