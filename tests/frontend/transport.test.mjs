import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join } from "node:path";
const { createProjectApi } = createRequire(import.meta.url)(join(process.env.FRONTEND_TEST_OUTPUT, "api/transport.js"));

test("manifest request preserves structured errors and never starts other work", async () => {
  const error = { code: "unsupported_version", messageKey: "errors.unsupportedProjectVersion", params: {}, retryable: false };
  const calls = [];
  const api = createProjectApi({ invoke: async (...args) => { calls.push(args); throw error; } });
  await assert.rejects(api.inspectManifest("/example/manifest.json"), (actual) => actual === error);
  assert.deepEqual(calls, [["project_inspect_manifest", { path: "/example/manifest.json" }]]);
});

test("disposing while listener registers releases it and ignores late events", async () => {
  let receive, finish;
  let stops = 0;
  const received = [];
  const api = createProjectApi({ listen: (_name, callback) => {
    receive = callback;
    return new Promise((resolve) => { finish = resolve; });
  } });
  const subscription = api.subscribe("active", (event) => received.push(event));
  receive({ version: 1, projectId: "other" });
  receive({ version: 2, projectId: "active" });
  receive({ version: 1, projectId: "active" });
  assert.equal(received.length, 1);
  subscription.dispose();
  receive({ version: 1, projectId: "active" });
  finish(() => { stops++; });
  await subscription.ready;
  subscription.dispose();
  assert.equal(stops, 1);
  assert.equal(received.length, 1);
});

test("listener registration failures remain observable", async () => {
  const error = new Error("disconnected");
  const api = createProjectApi({ listen: async () => { throw error; } });
  const subscription = api.subscribe("active", () => assert.fail("unexpected event"));
  await assert.rejects(subscription.ready, (actual) => actual === error);
  subscription.dispose();
});
