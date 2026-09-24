import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const require = createRequire(import.meta.url);
const { WorkspaceStore } = require(join(process.env.FRONTEND_TEST_OUTPUT, "state/workspace.js"));
const { JobStore } = require(join(process.env.FRONTEND_TEST_OUTPUT, "state/jobs.js"));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const project = id => ({ id, kind: "book" });
const view = id => ({ chapter: { id }, blocks: [{ chapterId: id }] });

test("rapid project switches never display a late chapter from the old project", async () => {
  const old = deferred();
  const store = new WorkspaceStore({
    open: async ({ projectId }) => project(projectId), settings: async () => ({}),
    chapters: async ({ projectId }) => ({ items: [{ id: projectId }], nextCursor: null }),
    chapter: ({ projectId }) => projectId === "old" ? old.promise : Promise.resolve(view(projectId)),
  });
  const first = store.open("old");
  await new Promise(resolve => setImmediate(resolve));
  await store.open("new");
  old.resolve(view("old")); await first;
  assert.equal(store.snapshot().project.id, "new");
  assert.equal(store.snapshot().chapter.chapter.id, "new");
  store.close(); assert.equal(store.snapshot().project, null);
});

test("out-of-order chapter responses cannot overwrite a newer selection", async () => {
  const a = deferred();
  const store = new WorkspaceStore({
    open: async () => project("p"), settings: async () => ({}),
    chapters: async () => ({ items: [{ id: "a" }, { id: "b" }], nextCursor: null }),
    chapter: ({ chapterId }) => chapterId === "a" ? a.promise : Promise.resolve(view("b")),
  });
  const opening = store.open("p"); await new Promise(resolve => setImmediate(resolve));
  await store.selectChapter("b"); a.resolve(view("a")); await opening;
  assert.equal(store.snapshot().chapter.chapter.id, "b");
});

test("job events survive workspace switches and reject stale large revision snapshots", async () => {
  const callbacks = new Map(), reads = [], old = deferred(); let disposed = 0;
  const item = (p, revision) => ({ job: { projectId: p, jobId: "j" }, revision, state: "running" });
  const store = new JobStore({
    subscribe: (p, cb) => { callbacks.set(p, cb); return { ready: Promise.resolve(), dispose: () => disposed++ }; },
    jobs: async () => [],
    job: ({ projectId }) => { reads.push(projectId); return projectId === "a" && reads.length === 1 ? old.promise : Promise.resolve(item(projectId, "9007199254740995")); },
  }, error => { throw error; });
  await store.watch("a"); await store.watch("b");
  const event = (p, seq) => ({ projectId: p, version: 1, jobId: "j", type: "job.updated", seq });
  callbacks.get("a")(event("a", "9007199254740993"));
  callbacks.get("a")(event("a", "9007199254740995"));
  callbacks.get("b")(event("b", "9007199254740995"));
  old.resolve(item("a", "9007199254740993"));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(store.list("a")[0].revision, "9007199254740995");
  assert.equal(store.list("b").length, 1);
  assert.equal(reads.filter(p => p === "a").length, 2);
  callbacks.get("a")(event("a", "9007199254740994"));
  assert.equal(reads.filter(p => p === "a").length, 2);
  store.dispose(); assert.equal(disposed, 2); assert.equal(store.list("a").length, 0);
});
