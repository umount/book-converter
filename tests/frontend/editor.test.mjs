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
