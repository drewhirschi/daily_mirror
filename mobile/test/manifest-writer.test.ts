import { test } from "node:test";
import assert from "node:assert/strict";
import { createManifestWriter } from "../src/cache/disk-cache";

/** Let every queued microtask settle before asserting. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("manifest writes run one at a time, in order", async () => {
  const started: string[] = [];
  const finished: string[] = [];
  const gates: (() => void)[] = [];
  const write = createManifestWriter(async (json) => {
    started.push(json);
    await new Promise<void>((resolve) => gates.push(resolve));
    finished.push(json);
  });

  write("first");
  const second = write("second");
  await settle();
  // Both writes share one temporary file, so overlapping them is exactly what
  // let one rename take the other's file and reject.
  assert.deepEqual(started, ["first"]);

  gates.shift()?.();
  await settle();
  assert.deepEqual(finished, ["first"]);
  assert.deepEqual(started, ["first", "second"]);

  gates.shift()?.();
  await second;
  assert.deepEqual(finished, ["first", "second"]);
});

test("a failed manifest write neither rejects nor stops later writes", async () => {
  const seen: string[] = [];
  const write = createManifestWriter(async (json) => {
    seen.push(json);
    if (json === "doomed") throw new Error("Destination already exists");
  });

  // An unhandled rejection here is what put a full-screen error over the app.
  await assert.doesNotReject(() => write("doomed"));
  await write("after");
  assert.deepEqual(seen, ["doomed", "after"]);
});
