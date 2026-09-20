import { test } from "node:test";
import assert from "node:assert/strict";
import { MIN_SPINNER_MS, runPullToRefresh } from "../src/pull-to-refresh";

/** Records the flag changes a RefreshControl would have seen, in order. */
const recorder = () => {
  const seen: boolean[] = [];
  return { seen, setRefreshing: (value: boolean) => void seen.push(value) };
};

const clock = (start = 0) => {
  let time = start;
  return {
    now: () => time,
    sleep: async (ms: number) => {
      time += ms;
    },
    advance: (ms: number) => {
      time += ms;
    },
  };
};

test("a fast refetch still leaves the spinner up long enough to animate out", async () => {
  const { seen, setRefreshing } = recorder();
  const time = clock();
  await runPullToRefresh(() => Promise.resolve(), {
    setRefreshing,
    now: time.now,
    sleep: time.sleep,
  });
  assert.deepEqual(seen, [true, false]);
  assert.equal(time.now(), MIN_SPINNER_MS);
});

test("a slow refetch does not add any extra delay", async () => {
  const { seen, setRefreshing } = recorder();
  const time = clock();
  await runPullToRefresh(
    async () => {
      time.advance(MIN_SPINNER_MS + 200);
    },
    { setRefreshing, now: time.now, sleep: time.sleep },
  );
  assert.deepEqual(seen, [true, false]);
  assert.equal(time.now(), MIN_SPINNER_MS + 200);
});

test("a failed refetch still brings the spinner down", async () => {
  const { seen, setRefreshing } = recorder();
  const time = clock();
  await runPullToRefresh(() => Promise.reject(new Error("offline")), {
    setRefreshing,
    now: time.now,
    sleep: time.sleep,
  });
  assert.deepEqual(seen, [true, false]);
});

test("the spinner is never raised by anything but the pull itself", async () => {
  const { seen, setRefreshing } = recorder();
  assert.deepEqual(seen, []);
  const time = clock();
  await runPullToRefresh(() => undefined, {
    setRefreshing,
    now: time.now,
    sleep: time.sleep,
    minimumMs: 0,
  });
  assert.deepEqual(seen, [true, false]);
});
