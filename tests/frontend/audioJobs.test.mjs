import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const require = createRequire(import.meta.url);
const { AudioJobStore } = require(join(process.env.FRONTEND_TEST_OUTPUT, "state/audioJobs.js"));
const tick = () => new Promise(resolve => setImmediate(resolve));
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const job = (projectId, id, state = "running") => ({
  projectId, id, state, voice: "Ryan", text: "original", device: "cpu", language: "Russian",
  completedChunks: 2, totalChunks: 10, completedChapters: 0, totalChapters: 2,
  currentChapter: "Chapter one", error: null, createdAt: "1000",
});

test("audio progress updates without a mounted narration tab and stays isolated by project", async t => {
  const items = [job("a", "j"), job("b", "k", "interrupted")];
  const store = new AudioJobStore({ audioList: async ({ projectId }) => structuredClone(items.filter(j => j.projectId === projectId)) }, assert.fail, 10);
  t.after(() => store.dispose());
  store.setProjects(["a", "b"]);
  await tick();
  assert.equal(store.list("a")[0].completedChunks, 2);
  assert.equal(store.list("b")[0].state, "interrupted");
  const version = store.snapshot();
  items[0].completedChunks = 7;
  items[0].completedChapters = 1;
  for (let attempt = 0; attempt < 100 && store.list("a")[0].completedChunks !== 7; attempt++) await delay(10);
  assert.equal(store.list("a")[0].completedChunks, 7);
  assert.equal(store.list("a")[0].completedChapters, 1);
  assert.ok(store.snapshot() > version);
  assert.equal(store.list("b")[0].completedChunks, 2);
  assert.equal(store.running(), true);
});

for (const action of ["start", "resume"]) test(`an older audio listing cannot undo ${action}`, async t => {
  const old = deferred();
  const paused = job("a", "j", "paused"), running = job("a", "j");
  let reads = 0;
  const calls = [];
  const store = new AudioJobStore({
    audioList: async () => ++reads === 2 ? old.promise : [paused],
    audioStart: async args => { calls.push(args); return running; },
    audioResume: async args => { calls.push(args); return running; },
  }, assert.fail, 60000);
  t.after(() => store.dispose());
  store.setProjects(["a"]);
  await tick();
  const stale = store.refresh("a");
  const args = action === "start" ? { projectId: "a", selection: { kind: "all" }, voice: "Ryan", device: "cpu", text: "original" } : { projectId: "a", jobId: "j" };
  await store[action](args);
  old.resolve([paused]);
  await stale;
  assert.equal(store.list("a")[0].state, "running");
  assert.deepEqual(calls, [args]);
});

test("clearing audio history preserves checkpoints and active jobs, persists, and allows resuming", async t => {
  const saved = new Map();
  const previous = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: key => saved.get(key) ?? null, setItem: (key, value) => saved.set(key, value),
  } });
  t.after(() => { if (previous) Object.defineProperty(globalThis, "localStorage", previous); else delete globalThis.localStorage; });
  const items = [job("a", "active"), job("a", "done", "succeeded"), job("a", "paused", "paused"), job("b", "done", "succeeded")];
  const api = {
    audioList: async ({ projectId }) => structuredClone(items.filter(j => j.projectId === projectId)),
    audioResume: async ({ projectId, jobId }) => {
      const current = items.find(j => j.projectId === projectId && j.id === jobId);
      current.state = "running";
      return structuredClone(current);
    },
  };
  let store = new AudioJobStore(api, assert.fail, 60000);
  t.after(() => store.dispose());
  store.setProjects(["a", "b"]); await tick();
  store.clearFinished("a");
  assert.deepEqual(store.list("a").map(j => j.id), ["active"]);
  assert.equal(store.list("a", true).length, 3);
  assert.equal(store.list("b").length, 1);
  store.dispose();
  store = new AudioJobStore(api, assert.fail, 60000);
  store.setProjects(["a", "b"]); await tick();
  assert.deepEqual(store.list("a").map(j => j.id), ["active"]);
  await store.resume({ projectId: "a", jobId: "paused" });
  assert.equal(store.list("a").find(j => j.id === "paused").state, "running");
  items.find(j => j.id === "paused").state = "paused";
  await store.refresh("a");
  assert.ok(store.list("a").some(j => j.id === "paused"));
});

test("pause reads fresh state after an in-flight listing and targets the requested audio job", async t => {
  const old = deferred();
  const running = job("a", "j");
  let reads = 0, cancelled;
  const store = new AudioJobStore({
    audioList: async () => ++reads === 2 ? old.promise : [{ ...running, state: cancelled ? "paused" : "running" }],
    audioCancel: async args => { cancelled = args; },
  }, assert.fail, 60000);
  t.after(() => store.dispose());
  store.setProjects(["a"]); await tick();
  const stale = store.refresh("a");
  const pausing = store.pause({ projectId: "a", jobId: "j" });
  await tick(); old.resolve([running]);
  await Promise.all([pausing, stale]);
  assert.deepEqual(cancelled, { projectId: "a", jobId: "j" });
  assert.equal(store.list("a")[0].state, "paused");
});

test("removed projects and disposed stores reject late responses and stop polling", async t => {
  const old = deferred(), pending = deferred();
  const reads = [];
  const store = new AudioJobStore({ audioList: ({ projectId }) => {
    reads.push(projectId); return projectId === "a" ? old.promise : pending.promise;
  } }, assert.fail, 10);
  store.setProjects(["a"]);
  store.setProjects(["b"]);
  old.resolve([job("a", "old")]); await tick();
  assert.deepEqual(store.list("a"), []);
  store.dispose();
  pending.resolve([job("b", "new")]); await tick();
  await delay(40);
  assert.deepEqual(store.list("b"), []);
  assert.deepEqual(reads, ["a", "b"]);
});

test("poll failures preserve known progress and report once until recovery", async t => {
  let failed = false;
  const errors = [];
  const store = new AudioJobStore({ audioList: async () => {
    if (failed) throw new Error("unavailable");
    return [job("a", "j")];
  } }, error => errors.push(error), 60000);
  t.after(() => store.dispose());
  store.setProjects(["a"]); await tick();
  failed = true;
  await store.refresh("a"); await store.refresh("a");
  assert.equal(errors.length, 1);
  assert.equal(store.list("a")[0].completedChunks, 2);
  failed = false; await store.refresh("a");
  failed = true; await store.refresh("a");
  assert.equal(errors.length, 2);
});
