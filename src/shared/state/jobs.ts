import type { JobRef, JobView, ProjectEvent } from "../contracts/generated";
import type { createProjectApi } from "../api/transport";
type Api = Pick<ReturnType<typeof createProjectApi>, "subscribe" | "jobs" | "job">;
const validRevision = (value: string) => /^(0|[1-9][0-9]*)$/.test(value);
const key = (job: JobRef) => `${job.projectId}/${job.jobId}`;

/** Background jobs remain isolated by project even when the visible workspace changes. */
export class JobStore {
  private values = new Map<string, JobView>();
  private subscriptions = new Map<string, ReturnType<Api["subscribe"]>>();
  private pending = new Map<string, Promise<void>>();
  private minimum = new Map<string, bigint>();
  private listeners = new Set<() => void>();
  private closed = false;
  constructor(private api: Api, private onError: (error: unknown) => void) {}
  subscribe(listener: () => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  list(projectId: string): JobView[] { return [...this.values.values()].filter(v => v.job.projectId === projectId); }
  private accept(job: JobView, expected: JobRef) {
    if (this.closed || job.job.projectId !== expected.projectId || job.job.jobId !== expected.jobId || !validRevision(job.revision)) return;
    const id = key(job.job), revision = BigInt(job.revision), previous = this.values.get(id);
    if (revision < (this.minimum.get(id) ?? 0n) || (previous && revision <= BigInt(previous.revision))) return;
    this.values.set(id, job);
    for (const listener of this.listeners) listener();
  }
  async watch(projectId: string): Promise<void> {
    if (this.closed) return;
    const existing = this.subscriptions.get(projectId);
    if (existing) return existing.ready;
    const subscription = this.api.subscribe(projectId, event => this.event(event));
    this.subscriptions.set(projectId, subscription);
    try {
      await subscription.ready;
      const jobs = await this.api.jobs({ projectId, cursor: null, limit: 100 });
      for (const job of jobs) this.accept(job, { projectId, jobId: job.job.jobId });
    } catch (error) {
      subscription.dispose();
      this.subscriptions.delete(projectId);
      if (!this.closed) this.onError(error);
    }
  }
  private event(event: ProjectEvent) {
    if (this.closed || event.version !== 1 || event.type !== "job.updated" || !event.jobId || !validRevision(event.seq)) return;
    const job = { projectId: event.projectId, jobId: event.jobId }, id = key(job), seq = BigInt(event.seq);
    if (seq <= (this.minimum.get(id) ?? -1n)) return;
    this.minimum.set(id, seq);
    void this.refresh(job).catch(error => { if (!this.closed) this.onError(error); });
  }
  refresh(job: JobRef): Promise<void> {
    if (this.closed) return Promise.resolve();
    const id = key(job), pending = this.pending.get(id);
    if (pending) return pending;
    const request = (async () => {
      // Repeat only if an event arrived during the read; never poll an unchanged job.
      let observed: bigint | undefined;
      do {
        observed = this.minimum.get(id);
        this.accept(await this.api.job(job), job);
      } while (!this.closed && observed !== this.minimum.get(id));
    })().finally(() => this.pending.delete(id));
    this.pending.set(id, request);
    return request;
  }
  dispose() {
    this.closed = true;
    for (const subscription of this.subscriptions.values()) subscription.dispose();
    this.subscriptions.clear(); this.listeners.clear(); this.values.clear(); this.minimum.clear();
  }
}
