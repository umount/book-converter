import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const require = createRequire(import.meta.url);
const { BookEditorSession } = require(join(process.env.FRONTEND_TEST_OUTPUT, "state/editor.js"));
const view = (revision = "1", text = "saved") => ({ chapter: { id: "c" }, translation: { id: `t${revision}`, revision }, blocks: [{id:"b",chapterId:"c",content:{kind:"text",text:"source"},translatedText:text}] });
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
test("typing while a save is in flight is serialized against the new translation snapshot", async () => {
  const first = deferred(), calls = []; let saved = view();
  const session = new BookEditorSession({chapter:async()=>saved, editTranslation:async args=>{ calls.push(args); if(calls.length===1) await first.promise; saved=view(String(calls.length+1),args.text); return saved.translation.revision; }}, "p", saved);
  session.edit("b","first"); const saving=session.flush(); session.edit("b","second"); assert.equal(session.flush(),saving); first.resolve(); await saving;
  assert.deepEqual(calls.map(c=>[c.translationId,c.expectedRevision,c.text]),[["t1","1","first"],["t2","2","second"]]);
  assert.equal(session.snapshot().drafts.size,0); assert.equal(session.snapshot().view.blocks[0].translatedText,"second"); session.dispose();
});
test("conflicts reject navigation flush and keep the draft until explicit discard", async () => {
  const conflict={code:"revision_conflict"}; const session=new BookEditorSession({chapter:async()=>view("3","external"),editTranslation:async()=>{throw conflict;}},"p",view());
  session.edit("b","local"); await assert.rejects(session.flush(),e=>e===conflict); assert.equal(session.snapshot().drafts.get("b"),"local"); assert.equal(session.snapshot().saving,false);
  await session.refresh(); assert.equal(session.snapshot().view.translation.revision,"1"); await session.discard(); assert.equal(session.snapshot().drafts.size,0); assert.equal(session.snapshot().view.translation.revision,"3"); session.dispose();
});
test("a refresh started before an edit cannot overwrite the subsequently saved snapshot", async () => {
  const old=deferred(); let count=0;
  const session=new BookEditorSession({chapter:()=>++count===1?old.promise:Promise.resolve(view("2","local")),editTranslation:async()=>"2"},"p",view());
  const refreshing=session.refresh(); session.edit("b","local"); await session.flush(); old.resolve(view()); await refreshing;
  assert.equal(session.snapshot().view.translation.revision,"2"); session.dispose();
});
test("a concurrent translation replacement after saving does not silently drop the local text", async () => {
  const session=new BookEditorSession({chapter:async()=>view("9","replacement"),editTranslation:async()=>"2"},"p",view());
  session.edit("b","local"); await assert.rejects(session.flush(),e=>e.code==="revision_conflict"); assert.equal(session.snapshot().drafts.get("b"),"local"); session.dispose();
});

test("title and body drafts serialize revisions and retain title typing during save", async () => {
  const first=deferred(),calls=[];let saved=view();saved.translation.title="Old title";
  const api={chapter:async()=>saved,
    editTitle:async args=>{calls.push(["title",args]);if(calls.length===1)await first.promise;const revision=String(Number(saved.translation.revision)+1);saved={...saved,translation:{...saved.translation,id:`t${revision}`,revision,title:args.title}};return revision;},
    editTranslation:async args=>{calls.push(["body",args]);const revision=String(Number(saved.translation.revision)+1);saved={...saved,translation:{...saved.translation,id:`t${revision}`,revision},blocks:saved.blocks.map(b=>({...b,translatedText:args.text}))};return revision;}};
  const session=new BookEditorSession(api,"p",saved);
  session.editTitle("First");const saving=session.flush();session.editTitle("Final");session.edit("b","Body");first.resolve();await saving;
  assert.deepEqual(calls.map(([kind,a])=>[kind,a.expectedRevision]),[["title","1"],["title","2"],["body","3"]]);
  assert.equal(session.snapshot().view.translation.title,"Final");assert.equal(session.snapshot().view.blocks[0].translatedText,"Body");assert.equal(session.snapshot().drafts.size,0);session.dispose();
});

test("failed title save retains the title draft until explicit discard", async () => {
 const session=new BookEditorSession({chapter:async()=>view("3"),editTitle:async()=>{throw {code:"revision_conflict"};}},"p",view());
 session.editTitle("Unsaved title");await assert.rejects(session.flush());assert.equal(session.snapshot().drafts.size,1);await session.refresh();assert.equal(session.snapshot().view.translation.revision,"1");await session.discard();assert.equal(session.snapshot().drafts.size,0);session.dispose();
});

test("out-of-order refresh responses cannot restore an older translation", async () => {
  const older = deferred(), newer = deferred(); let calls = 0;
  const session = new BookEditorSession({ chapter: () => ++calls === 1 ? older.promise : newer.promise }, "p", view());
  const first = session.refresh(), second = session.refresh();
  newer.resolve(view("3", "newest")); await second;
  older.resolve(view("2", "older")); await first;
  assert.equal(session.snapshot().view.translation.revision, "3");
  assert.equal(session.snapshot().view.blocks[0].translatedText, "newest");
  session.dispose();
});

test("discard response preserves edits entered while the chapter is loading", async () => {
  const loaded = deferred();
  const session = new BookEditorSession({ chapter: () => loaded.promise, editTranslation: async () => { throw { code: "revision_conflict" }; } }, "p", view());
  session.edit("b", "old draft"); await assert.rejects(session.flush());
  const discarding = session.discard();
  session.edit("b", "new draft"); session.editTitle("new title");
  loaded.resolve(view("2", "external")); await discarding;
  assert.equal(session.snapshot().drafts.get("b"), "new draft");
  assert.equal(session.snapshot().drafts.get("$title"), "new title");
  assert.equal(session.snapshot().view.translation.revision, "1");
  assert.equal(session.snapshot().error.code, "revision_conflict");
  session.dispose();
});

test("discard cancels scheduled autosave before waiting for the reload", async () => {
  const loaded = deferred(); let saves = 0;
  const session = new BookEditorSession({ chapter: () => loaded.promise, editTranslation: async () => { ++saves; return "2"; } }, "p", view());
  session.edit("b", "discarded draft");
  const discarding = session.discard();
  await new Promise(resolve => setTimeout(resolve, 700));
  assert.equal(saves, 0);
  loaded.resolve(view("2", "external")); await discarding;
  assert.equal(session.snapshot().drafts.size, 0);
  assert.equal(session.snapshot().view.blocks[0].translatedText, "external");
  session.dispose();
});

test("a delayed discard cannot replace the result of an explicit save retry", async () => {
  const loaded = deferred(); let reads = 0, writes = 0;
  const session = new BookEditorSession({
    chapter: () => ++reads === 1 ? loaded.promise : Promise.resolve(view("2", "local")),
    editTranslation: async () => { if (++writes === 1) throw { code: "revision_conflict" }; return "2"; },
  }, "p", view());
  session.edit("b", "local"); await assert.rejects(session.flush());
  const discarding = session.discard();
  await session.flush();
  loaded.resolve(view("1", "old")); await discarding;
  assert.equal(session.snapshot().view.translation.revision, "2");
  assert.equal(session.snapshot().view.blocks[0].translatedText, "local");
  assert.equal(session.snapshot().drafts.size, 0);
  session.dispose();
});
