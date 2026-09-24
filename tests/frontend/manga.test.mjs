import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const { fittedWidth, pageKeyDelta } = createRequire(import.meta.url)(join(process.env.FRONTEND_TEST_OUTPUT, "state/mangaCanvas.js"));

test("whole-page fit respects both viewport axes without changing pixel zoom", () => {
  assert.equal(fittedWidth(600, 900, 1000, 600, "page"), 400);
  assert.equal(fittedWidth(1200, 600, 1000, 600, "page"), 1000);
  assert.equal(fittedWidth(600, 900, 1000, 600, "fit"), 1000);
  assert.equal(fittedWidth(600, 900, 1000, 600, "150"), 900);
  assert.equal(fittedWidth(600, 900, 0, 0, "page"), 0);
});

test("reading direction reverses horizontal navigation, not sequential paging", () => {
  assert.equal(pageKeyDelta("ArrowLeft", true), 1);
  assert.equal(pageKeyDelta("ArrowLeft", false), -1);
  assert.equal(pageKeyDelta("ArrowRight", true), -1);
  assert.equal(pageKeyDelta("ArrowRight", false), 1);
  for (const rtl of [true, false]) {
    assert.equal(pageKeyDelta("PageDown", rtl), 1);
    assert.equal(pageKeyDelta("PageUp", rtl), -1);
    assert.equal(pageKeyDelta("ArrowDown", rtl), 0);
  }
});
