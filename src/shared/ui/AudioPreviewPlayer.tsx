import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { projectApi as api } from "../api/projects";
import { previewMode } from "../api/desktop";
import type { AudioJobView, AudioPreviewView } from "../contracts/generated";
import { errorText, type T } from "../../app/strings";

export function AudioPreviewPlayer({ job, t }: { job: AudioJobView; t: T }) {
  const [sample, setSample] = useState<AudioPreviewView | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState<unknown>(null);
  const [saved, setSaved] = useState(false);
  const alive = useRef(true), lock = useRef(false), player = useRef<HTMLAudioElement>(null);
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; };
  }, []);
  useEffect(() => {
    if (sample) void player.current?.play().catch(() => { /* Native controls remain available if autoplay is blocked. */ });
  }, [sample]);
  async function act(action: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError(null);
    try { await action(); }
    catch (e) { if (alive.current) setError(e); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  return <div className="bc-audio-preview">
    <div className="bc-toolbar">
      <button disabled={busy || job.completedChunks === 0} onClick={() => void act(async () => {
        const next = await api.audioPreview({ projectId: job.projectId, jobId: job.id });
        if (alive.current) { setSample(next); setSaved(false); }
      })}>{busy ? t("loading") : t(sample ? "audioPreviewRefresh" : "audioListen")}</button>
      {sample && <button disabled={busy || previewMode || saved} onClick={() => void act(async () => {
        const destination = await open({ directory: true, multiple: false });
        if (typeof destination !== "string") return;
        await api.audioPreviewExport({ projectId: job.projectId, jobId: job.id, previewId: sample.previewId, destination });
        if (alive.current) setSaved(true);
      })}>{t("audioPreviewSave")}</button>}
    </div>
    <p className="bc-hint">{t(job.completedChunks ? "audioPreviewHint" : "audioPreviewWaiting")}</p>
    {sample && <>
      <span className="bc-hint">{sample.title}</span>
      <audio ref={player} controls preload="metadata" src={sample.audioUrl} aria-label={t("audioListen")}
        onError={() => setError(t("audioPlaybackError"))}
        onPlay={event => document.querySelectorAll("audio").forEach(audio => { if (audio !== event.currentTarget) audio.pause(); })} />
    </>}
    {saved && <span className="bc-success" role="status">{t("audioPreviewSaved")}</span>}
    {error != null && <p className="bc-error" role="alert">{errorText(error, t)}</p>}
  </div>;
}
