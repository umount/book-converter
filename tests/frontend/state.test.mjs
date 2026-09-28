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

test("clearing jobs keeps active jobs and other projects; resumed jobs reappear", async () => {
  const items = [
    { job: { projectId: "p", jobId: "done" }, revision: "1", state: "succeeded" },
    { job: { projectId: "p", jobId: "failed" }, revision: "1", state: "failed" },
    { job: { projectId: "p", jobId: "active" }, revision: "1", state: "running" },
    { job: { projectId: "q", jobId: "done" }, revision: "1", state: "succeeded" },
  ];
  const store = new JobStore({
    subscribe: () => ({ ready: Promise.resolve(), dispose() {} }),
    jobs: async ({ projectId }) => items.filter(j => j.job.projectId === projectId),
    job: async ({ jobId }) => items.find(j => j.job.jobId === jobId),
  }, error => { throw error; });
  await store.watch("p"); await store.watch("q");
  store.clearFinished("p");
  assert.deepEqual(store.list("p").map(j => j.job.jobId), ["active"]);
  assert.equal(store.list("q").length, 1);
  await store.refresh(items[1].job);
  assert.equal(store.list("p").length, 1);
  items[1] = { ...items[1], revision: "2", state: "running" };
  await store.refresh(items[1].job);
  assert.equal(store.list("p").length, 2);
  store.dispose();
});

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

test("chapter list refresh preserves the open editor and cannot overwrite a newer local summary", async () => {
  const delayed=deferred();let reads=0;
  const store=new WorkspaceStore({open:async()=>project("p"),settings:async()=>({}),chapter:async()=>view("a"),chapters:async()=> ++reads===2 ? delayed.promise : {items:[{id:"a",status:"pending"}],nextCursor:null}});
  await store.open("p");const editorView=store.snapshot().chapter;
  const refresh=store.refreshChapters();store.updateChapter({id:"a",status:"done",origin:"manual"});
  delayed.resolve({items:[{id:"a",status:"failed"}],nextCursor:null});await refresh;
  assert.equal(store.snapshot().chapters[0].status,"done");assert.equal(store.snapshot().chapter,editorView);
  await store.refreshChapters();assert.equal(store.snapshot().chapter,editorView);
});

test("chapter list refresh from a closed project is discarded", async () => {
  const delayed=deferred();let reads=0;
  const store=new WorkspaceStore({open:async()=>project("p"),settings:async()=>({}),chapter:async()=>view("a"),chapters:async()=> ++reads===2 ? delayed.promise : {items:[{id:"a"}],nextCursor:null}});
  await store.open("p");const refresh=store.refreshChapters();store.close();
  delayed.resolve({items:[{id:"a",status:"done"}],nextCursor:null});await refresh;
  assert.equal(store.snapshot().project,null);assert.deepEqual(store.snapshot().chapters,[]);
});

test("removing the selected chapter opens its next neighbour and handles an empty book", async () => {
  let chapters = [{id:"a",position:0},{id:"b",position:1},{id:"c",position:2}];
  const reads=[];
  const store=new WorkspaceStore({
    open:async()=>project("p"),settings:async()=>({}),
    chapters:async()=>({items:chapters,nextCursor:null}),
    chapter:async({chapterId})=>{reads.push(chapterId);return {chapter:{...chapters.find(c=>c.id===chapterId)},blocks:[]};},
  });
  await store.open("p");await store.selectChapter("b");
  chapters=[{id:"a",position:0},{id:"c",position:1}];
  await store.refreshChapters();
  assert.equal(store.snapshot().chapter.chapter.id,"c");
  chapters=[{id:"a",position:0}];await store.refreshChapters();
  assert.equal(store.snapshot().chapter.chapter.id,"a");
  chapters=[];await store.refreshChapters();
  assert.equal(store.snapshot().chapter,null);
  assert.deepEqual(store.snapshot().chapters,[]);
  assert.equal(store.snapshot().loading,false);
  assert.deepEqual(reads,["a","b","c","a"]);
  store.dispose();
});
