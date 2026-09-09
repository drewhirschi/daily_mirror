import { test } from "node:test";
import assert from "node:assert/strict";
import { selectedFrameIndex } from "../src/flipbook";

const frames = (...days: string[]) =>
  days.map((capture_day) => ({ capture_day }));

test("refresh follows the latest matched day as new frames arrive", () => {
  assert.equal(selectedFrameIndex(frames("2026-09-06")), 0);
  assert.equal(selectedFrameIndex(frames("2026-09-06", "2026-09-08")), 1);
});

test("refresh preserves a scrubbed day when earlier frames are backfilled", () => {
  const updated = frames("2026-09-01", "2026-09-06", "2026-09-08");
  assert.equal(selectedFrameIndex(updated, "2026-09-06"), 1);
  assert.equal(selectedFrameIndex(updated, "2026-09-05"), 2);
  assert.equal(selectedFrameIndex([], "2026-09-06"), -1);
});
