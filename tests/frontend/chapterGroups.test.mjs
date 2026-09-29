import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const require = createRequire(import.meta.url);
const { chapterRows } = require(join(process.env.FRONTEND_TEST_OUTPUT, "state/chapterGroups.js"));
const chapters = [{ id: "intro", volume: null }, { id: "a", volume: "第1集" }, { id: "b", volume: "第1集" }, { id: "c", volume: "第2集" }];
test("volume collapse preserves order and filtering reveals matches", () => {
  const open = chapterRows(chapters, new Set(), "p");
  assert.deepEqual(open.filter(r => r.kind === "chapter").map(r => r.chapter.id), ["intro", "a", "b", "c"]);
  assert.deepEqual(open.filter(r => r.kind === "volume").map(r => r.count), [2, 1]);
  const closed = new Set(["p/第1集"]);
  assert.deepEqual(chapterRows(chapters, closed, "p").filter(r => r.kind === "chapter").map(r => r.chapter.id), ["intro", "c"]);
  assert.equal(chapterRows(chapters, closed, "p", true).length, open.length);
  assert.equal(chapterRows(chapters, closed, "other").length, open.length);
});
test("books without volumes keep a flat list", () => {
  assert.deepEqual(chapterRows([{id:"a"},{id:"b"}], new Set(), "p").map(r => r.kind), ["chapter", "chapter"]);
});
test("translated volume label preserves the source key and grouping", () => {
  const rows=chapterRows([{id:"a",volume:"第1集",translatedVolume:"Том 1"}],new Set(),"p");
  assert.equal(rows[0].label,"Том 1");
  assert.equal(rows[0].source,"第1集");
  assert.equal(rows[0].key,"p/第1集");
});
