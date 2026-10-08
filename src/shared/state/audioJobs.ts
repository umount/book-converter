import type { createProjectApi } from "../api/transport";
import type { AudioJobArgs, AudioJobView, AudioStartArgs } from "../contracts/generated";

type Api = Pick<ReturnType<typeof createProjectApi>, "audioList" | "audioStart" | "audioResume" | "audioCancel">;
type Watch = {
  jobs: AudioJobView[];
  generation: number;
  pending?: Promise<void>;
  timer?: ReturnType<typeof setTimeout>;
  failed: boolean;
};
const storageKey = "book-converter.hidden-audio-jobs";
const key = (job: AudioJobView) => `${job.projectId}/${job.id}`;
const signature = (job: AudioJobView) => JSON.stringify(job);
const active = (job: AudioJobView) => job.state === "running" || job.state === "pausing";

/** Audio jobs stay observable when the Narration tab is unmounted. */
export class AudioJobStore {
  private watches = new Map<string, Watch>();
  private listeners = new Set<() => void>();
  private hidden = new Map<string, string>();
  private version = 0;
  private closed = false;

  constructor(private api: Api, private onError: (error: unknown) => void, private interval = 1500) {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "[]");
      if (Array.isArray(saved)) for (const item of saved) {
        if (Array.isArray(item) && typeof item[0] === "string" && typeof item[1] === "string") {
          this.hidden.set(item[0], item[1]);
        }
      }
    } catch { /* History clearing remains available without browser storage. */ }
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };
  snapshot = () => this.version;
  list(projectId: string, includeHidden = false): AudioJobView[] {
    return (this.watches.get(projectId)?.jobs ?? []).filter(job =>
      includeHidden || active(job) || this.hidden.get(key(job)) !== signature(job));
  }
  running(): boolean {
    return [...this.watches.values()].some(watch => watch.jobs.some(active));
  }
  private notify() {
    this.version++;
    for (const listener of this.listeners) listener();
  }
  private persistHidden() {
    try { localStorage.setItem(storageKey, JSON.stringify([...this.hidden])); }
    catch { /* Keep session-only clearing available. */ }
  }
  clearFinished(projectId: string) {
    for (const job of this.list(projectId, true)) {
      if (!active(job)) this.hidden.set(key(job), signature(job));
    }
    this.persistHidden();
    this.notify();
  }
  setProjects(projectIds: string[]) {
    if (this.closed) return;
    const selected = new Set(projectIds);
    let removed = false;
    for (const [id, watch] of this.watches) {
      if (!selected.has(id)) {
        clearTimeout(watch.timer);
        this.watches.delete(id);
        removed = true;
      }
    }
    for (const id of selected) this.watch(id);
    if (removed) this.notify();
  }
  private watch(projectId: string): Watch {
    let watch = this.watches.get(projectId);
    if (!watch) {
      watch = { jobs: [], generation: 0, failed: false };
      this.watches.set(projectId, watch);
      void this.refresh(projectId);
    }
    return watch;
  }
  private publish(projectId: string, watch: Watch, jobs: AudioJobView[]) {
    if (this.closed || this.watches.get(projectId) !== watch) return;
    const next = jobs.filter(job => job.projectId === projectId)
      .sort((a, b) => Number(b.createdAt) - Number(a.createdAt) || a.id.localeCompare(b.id));
    let changed = false;
    for (const job of next) {
      const hidden = this.hidden.get(key(job));
      if (hidden !== undefined && hidden !== signature(job)) {
        this.hidden.delete(key(job));
        changed = true;
      }
    }
    if (changed) this.persistHidden();
    if (JSON.stringify(watch.jobs) !== JSON.stringify(next)) {
      watch.jobs = next;
      this.notify();
    }
  }
  async refresh(projectId: string): Promise<void> {
    const watch = this.watches.get(projectId);
    if (this.closed || !watch) return;
    // An explicit refresh after a mutation must read after any older request.
    if (watch.pending) {
      await watch.pending;
      if (this.watches.get(projectId) === watch) await this.refresh(projectId);
      return;
    }
    clearTimeout(watch.timer);
    const generation = watch.generation;
    watch.pending = (async () => {
      try {
        const jobs = await this.api.audioList({ projectId });
        if (watch.generation === generation) this.publish(projectId, watch, jobs);
        watch.failed = false;
      } catch (error) {
        if (!this.closed && this.watches.get(projectId) === watch && !watch.failed) this.onError(error);
        watch.failed = true;
      }
    })();
    await watch.pending;
    watch.pending = undefined;
    if (!this.closed && this.watches.get(projectId) === watch) {
      watch.timer = setTimeout(() => void this.refresh(projectId), this.interval);
    }
  }
  private accept(job: AudioJobView, watch: Watch) {
    if (this.closed || this.watches.get(job.projectId) !== watch) return;
    watch.generation++;
    this.hidden.delete(key(job));
    this.persistHidden();
    this.publish(job.projectId, watch, [job, ...watch.jobs.filter(previous => previous.id !== job.id)]);
  }
  async start(args: AudioStartArgs): Promise<AudioJobView> {
    const watch = this.watch(args.projectId);
    const job = await this.api.audioStart(args);
    this.accept(job, watch);
    return job;
  }
  async resume(args: AudioJobArgs): Promise<AudioJobView> {
    const watch = this.watch(args.projectId);
    const job = await this.api.audioResume(args);
    this.accept(job, watch);
    return job;
  }
  async pause(args: AudioJobArgs): Promise<void> {
    await this.api.audioCancel(args);
    await this.refresh(args.projectId);
  }
  dispose() {
    this.closed = true;
    for (const watch of this.watches.values()) clearTimeout(watch.timer);
    this.watches.clear();
    this.listeners.clear();
  }
}
