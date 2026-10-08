import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { projectApi as api } from "../../shared/api/projects";
import { previewMode } from "../../shared/api/desktop";
import type { AudioDevice, AudioSetupView, AudioText, ChapterSummary, EntitySelection, ProjectDescriptor } from "../../shared/contracts/generated";
import type { BookEditorSession } from "../../shared/state/editor";
import type { AudioJobStore } from "../../shared/state/audioJobs";
import { errorText, type T } from "../../app/strings";

const voices = ["Ryan", "Aiden", "Serena", "Vivian", "Uncle_Fu", "Dylan", "Eric", "Ono_Anna", "Sohee"];
const size = (bytes: number) => `${(bytes / 1_000_000_000).toFixed(2)} GB`;
export function BookNarration({ project, chapters, session, t, audio, onAudioJob }: {
  project: ProjectDescriptor; chapters: ChapterSummary[]; session: BookEditorSession | null; t: T;
  audio: AudioJobStore; onAudioJob: () => void;
}) {
  const [setup, setSetup] = useState<AudioSetupView | null>(null);
  const jobs = audio.list(project.id, true);
  const [voice, setVoice] = useState("Ryan"), [device, setDevice] = useState<AudioDevice>("auto");
  const [text, setText] = useState<AudioText>("translation");
  const [scope, setScope] = useState(session ? "chapter" : "all");
  const [first, setFirst] = useState(chapters[0]?.id ?? ""), [last, setLast] = useState(chapters[chapters.length - 1]?.id ?? "");
  const [busy, setBusy] = useState(false), [error, setError] = useState<unknown>(null);
  const [exported, setExported] = useState("");
  const alive = useRef(true), lock = useRef(false);
  useEffect(() => {
    alive.current = true;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const nextSetup = await api.audioSetup();
        if (!disposed) setSetup(nextSetup);
      } catch (e) { if (!disposed) setError(e); }
      finally { if (!disposed) timer = setTimeout(() => void poll(), 1500); }
    }
    void poll();
    return () => { disposed = true; alive.current = false; clearTimeout(timer); };
  }, [project.id]);
  async function act(action: () => Promise<unknown>) {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError(null);
    try { await action(); }
    catch (e) { if (alive.current) setError(e); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  const ready = !!setup?.files.length && setup.files.every(f => f.status === "downloaded");
  const downloaded = setup?.files.reduce((sum, f) => sum + f.downloadedBytes, 0) ?? 0;
  const total = setup?.files.reduce((sum, f) => sum + f.model.bytes, 0) ?? 1;
  const failed = setup?.files.find(f => f.failure);
  const running = audio.running();
  const loadingModel = setup?.engine.state === "loading";
  const loadedModel = setup?.engine.state === "ready";
  const matchingModel = loadedModel && (device === "auto" || setup?.engine.device === device);
  const chapterId = session?.snapshot().view.chapter.id;
  const validRange = chapters.findIndex(c => c.id === first) >= 0 && chapters.findIndex(c => c.id === first) <= chapters.findIndex(c => c.id === last);
  async function start() {
    await session?.flush();
    let selection: EntitySelection = { kind: "all" };
    if (scope === "chapter") { if (!chapterId) return; selection = { kind: "explicit_ids", ids: [chapterId] }; }
    if (scope === "range") selection = { kind: "range", first, last };
    await audio.start({ projectId: project.id, selection, voice, text, device });
    onAudioJob();
  }
  return <div className="bc-tool bc-narration">
    <div className="bc-audio-heading"><h2>{t("narration")}</h2><span className="bc-audio-format">MP3 · 128 kbps</span></div>
    <p className="bc-hint">{t("audioIntro")}</p>
    <section className="bc-audio-setup" aria-label={t("audioModel")}>
      <div className="bc-audio-heading"><h3>Qwen3-TTS 0.6B</h3><span className={ready ? "bc-success" : "bc-hint"}>{ready ? t("audioReady") : t("audioModel")}</span></div>
      {!setup ? <p role="status">{t("loading")}</p> : <>
        {!setup.runtimeReady && <p className="bc-error">{t("audioRuntimeMissing")}</p>}
        {!ready && <>
          <progress value={downloaded} max={total} aria-label={t("audioDownload")} />
          <div className="bc-audio-heading"><span className="bc-hint">{size(downloaded)} / {size(total)}</span>
            <button disabled={busy} onClick={() => void act(async () => {
              if (setup.downloading) await api.audioPauseDownload(); else await api.audioDownload();
              const next = await api.audioSetup();
              if (alive.current) setSetup(next);
            })}>{setup.downloading ? t("audioPause") : t("audioDownload")}</button>
          </div>
          {failed && <p role="alert" className="bc-error">{t("audioDownloadFailed")} ({failed.failure})</p>}
          {setup.files.some(f => f.status === "verifying") && <p role="status">{t("audioVerifying")}</p>}
        </>}
        <div className="bc-audio-engine">
          <div className="bc-audio-heading"><strong>{t("audioEngine")}</strong><span role="status" className={loadedModel ? "bc-success" : "bc-hint"}>
            {t(({ unloaded: "audioEngineUnloaded", loading: "audioEngineLoading", ready: "audioEngineReady", failed: "audioFailed" } as const)[setup.engine.state])}
            {setup.engine.device && setup.engine.device !== "auto" ? ` · ${setup.engine.device.toUpperCase()}` : ""}
          </span></div>
          <p className="bc-hint">{t("audioEngineHint")}</p>
          <div className="bc-toolbar">
            <button disabled={busy || running || loadingModel || matchingModel || !ready || !setup.runtimeReady} onClick={() => void act(async () => {
              await api.audioLoad({ device });
              const next = await api.audioSetup();
              if (alive.current) setSetup(next);
            })}>{t("audioLoad")}</button>
            <button disabled={busy || running || loadingModel || !loadedModel} onClick={() => void act(async () => {
              await api.audioUnload();
              const next = await api.audioSetup();
              if (alive.current) setSetup(next);
            })}>{t("audioUnload")}</button>
          </div>
          {setup.engine.error && <p role="alert" className="bc-error">{errorText(setup.engine.error, t)}</p>}
        </div>
      </>}
    </section>
    <fieldset className="bc-audio-options" disabled={busy || running || loadingModel}>
      <legend>{t("audioOptions")}</legend>
      <div className="bc-audio-fields">
        <label>{t("audioText")}<select value={text} onChange={e => setText(e.target.value as AudioText)}><option value="translation">{t("audioTranslation")}</option><option value="original">{t("audioOriginal")}</option></select></label>
        <label>{t("selection")}<select value={scope} onChange={e => setScope(e.target.value)}><option value="chapter" disabled={!chapterId}>{t("selectedChapter")}</option><option value="range">{t("audioRange")}</option><option value="all">{t("allChapters")}</option></select></label>
        {scope === "range" && <>
          <label>{t("audioFrom")}<select value={first} onChange={e => setFirst(e.target.value)}>{chapters.map(c => <option key={c.id} value={c.id}>{c.position + 1}. {c.translatedTitle || c.title}</option>)}</select></label>
          <label>{t("audioTo")}<select value={last} onChange={e => setLast(e.target.value)}>{chapters.map(c => <option key={c.id} value={c.id}>{c.position + 1}. {c.translatedTitle || c.title}</option>)}</select></label>
        </>}
        <label>{t("audioVoice")}<select value={voice} onChange={e => setVoice(e.target.value)}>{voices.map(v => <option key={v}>{v}</option>)}</select></label>
        <label>{t("audioDevice")}<select value={device} onChange={e => setDevice(e.target.value as AudioDevice)}><option value="auto">{t("audioAuto")}</option><option value="cpu">CPU</option><option value="cuda">NVIDIA CUDA</option></select></label>
      </div>
      <p className="bc-hint">{t("audioCpuHint")}</p>
      <button className="primary" disabled={!ready || !setup?.runtimeReady || !chapters.length || (scope === "chapter" && !chapterId) || (scope === "range" && !validRange)} onClick={() => void act(start)}>{t("audioStart")}</button>
    </fieldset>
    {error != null && <p role="alert" className="bc-error">{errorText(error, t)}</p>}
    <section className="bc-audio-jobs" aria-label={t("audioJobs")}>
      <h3>{t("audioJobs")}</h3>
      {!jobs.length && <p className="bc-hint">{t("audioEmptyJobs")}</p>}
      {jobs.map(job => <article key={job.id} className="bc-audio-job">
        <div className="bc-audio-heading"><strong>{job.voice} · {t(job.text === "translation" ? "audioTranslation" : "audioOriginal")}</strong><span>{t(({ running: "audioRunning", pausing: "audioPausing", paused: "audioPaused", interrupted: "audioInterrupted", failed: "audioFailed", succeeded: "audioFinished" } as const)[job.state])}</span></div>
        <p className="bc-hint">{t("chapters")}: {job.completedChapters} / {job.totalChapters} · {job.language} · {new Date(Number(job.createdAt)).toLocaleString()}</p>
        <progress value={job.completedChunks} max={Math.max(1, job.totalChunks)} aria-label={t("audioProgress")} />
        <p className="bc-audio-current" role="status">{job.currentChapter || t("audioPreparing")} <span className="bc-hint">{job.completedChunks} / {job.totalChunks}</span></p>
        {job.error && <p className="bc-error" role="alert">{errorText(job.error, t)}</p>}
        <div className="bc-toolbar">
          {job.state === "running" ? <button disabled={busy} onClick={() => void act(() => audio.pause({ projectId: project.id, jobId: job.id }))}>{t("audioPause")}</button>
            : job.state === "pausing" ? <span className="bc-hint">{t("audioPauseHint")}</span>
            : job.state !== "succeeded" ? <button disabled={busy || running || !ready || !setup?.runtimeReady} onClick={() => void act(async () => { await audio.resume({ projectId: project.id, jobId: job.id }); onAudioJob(); })}>{t("audioResume")}</button>
            : <button disabled={busy || previewMode} onClick={() => void act(async () => {
              const destination = await open({ directory: true, multiple: false });
              if (typeof destination === "string") { const path = await api.audioExport({ projectId: project.id, jobId: job.id, destination }); if (alive.current) setExported(path); }
            })}>{t("audioSave")}</button>}
        </div>
      </article>)}
    </section>
    {exported && <p className="bc-success" role="status">{t("audioSaved")} <button onClick={() => void act(() => openPath(exported))}>{t("audioOpenFolder")}</button></p>}
  </div>;
}
