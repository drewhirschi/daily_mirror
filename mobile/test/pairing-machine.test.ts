import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError } from "@daily-mirror/api";
import {
  awaitConfirmation,
  initialPairingState,
  pairingReducer,
  provisionDevice,
  stepErrorMessage,
  failureMessage,
  type PairingEvent,
  type PairingState,
} from "../src/pairing/machine";
import type { PairingDevice } from "../src/pairing/provisioning";
import type {
  ClaimTokenGrant,
  ProvisioningPayload,
  ProvisioningResult,
} from "../src/pairing/contract";

/** A stand-in for the Espressif library's device handle. */
function fakeDevice(
  results: ProvisioningResult[],
  overrides: Partial<PairingDevice> = {},
) {
  const sent: {
    payload?: ProvisioningPayload;
    wifi?: [string, string];
    reads: number;
  } = { reads: 0 };
  const device: PairingDevice = {
    name: "mirror-4f2a",
    connect: async () => undefined,
    scanWifi: async () => [{ ssid: "Home", rssi: -40, open: false }],
    async sendPayload(payload) {
      sent.payload = payload;
      return { status: "awaiting_confirm" };
    },
    async sendWifiCredentials(ssid, passphrase) {
      assert.ok(sent.payload, "the claim token must reach the device first");
      sent.wifi = [ssid, passphrase];
    },
    async readResult() {
      sent.reads += 1;
      const next = results.shift();
      if (!next) throw new Error("no more replies");
      return next;
    },
    disconnect: () => undefined,
    ...overrides,
  };
  return { device, sent };
}

const run = (
  events: PairingEvent[],
  from: PairingState = initialPairingState,
) => events.reduce(pairingReducer, from);

test("the BLE happy path opens on device selection and walks to success", () => {
  const { device } = fakeDevice([]);
  assert.equal(initialPairingState.step, "discover");
  let state = run([{ type: "begin", needsManualJoin: false }]);
  assert.deepEqual([state.step, state.busy], ["discover", true]);
  state = run([{ type: "found", devices: [device] }], state);
  assert.deepEqual(
    [state.step, state.busy, state.found.length],
    ["discover", false, 1],
  );
  state = run([{ type: "select", device }], state);
  assert.deepEqual(
    [state.step, state.busy, state.device],
    ["wifi", true, device],
  );
  state = run(
    [
      {
        type: "networks",
        networks: [{ ssid: "Home", rssi: -40, open: false }],
      },
      { type: "ssid", ssid: "Home" },
      { type: "send" },
    ],
    state,
  );
  assert.deepEqual(
    [state.step, state.stage, state.attempt],
    ["sending", "token", 1],
  );
  state = run([{ type: "awaiting", now: 1_000 }], state);
  assert.equal(state.step, "confirm");
  assert.equal(state.confirmDeadline, 31_000);
  state = run(
    [{ type: "result", result: { status: "claimed", device_name: "Hallway" } }],
    state,
  );
  assert.deepEqual(
    [state.step, state.deviceName, state.busy],
    ["success", "Hallway", false],
  );
});

test("the SoftAP fallback still inserts the manual join step", () => {
  let state = run([{ type: "begin", needsManualJoin: true }]);
  assert.deepEqual([state.step, state.busy], ["join", false]);
  state = run([{ type: "joined" }], state);
  assert.deepEqual([state.step, state.busy], ["discover", true]);
});

test("awaiting_confirm keeps the confirm step waiting", () => {
  const state = run([
    { type: "begin", needsManualJoin: false },
    { type: "awaiting", now: 0 },
    { type: "result", result: { status: "awaiting_confirm" } },
  ]);
  assert.deepEqual([state.step, state.busy], ["confirm", true]);
});

test("a missed button press offers a retry that replays the send step", () => {
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "send" },
    { type: "awaiting", now: 0 },
    { type: "timeout" },
  ]);
  assert.equal(state.step, "confirm");
  assert.equal(state.busy, false);
  assert.match(state.error, /button press in time/);
  assert.equal(state.retryStep, "sending");
  state = run([{ type: "retry" }], state);
  assert.deepEqual(
    [state.step, state.busy, state.stage, state.attempt],
    ["sending", true, "token", 2],
  );
});

test("a device-reported failure retries from the send step with plain words", () => {
  const state = run([
    { type: "begin", needsManualJoin: false },
    { type: "send" },
    { type: "awaiting", now: 0 },
    { type: "result", result: { status: "failed", reason: "wifi_auth" } },
  ]);
  assert.match(state.error, /could not join that Wi-Fi network/);
  assert.equal(state.retryStep, "sending");
  assert.equal(
    failureMessage("claim_token_expired"),
    failureMessage("expired"),
  );
  assert.match(failureMessage("weird"), /could not finish pairing/);
});

test("a failed scan returns to discover and can be scanned again", () => {
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "failed", message: "No camera answered.", retryStep: "discover" },
  ]);
  assert.deepEqual(
    [state.step, state.busy, state.error],
    ["discover", false, "No camera answered."],
  );
  state = run([{ type: "retry" }], state);
  assert.deepEqual(
    [state.step, state.busy, state.error],
    ["discover", true, ""],
  );
});

test("back returns from Wi-Fi to device selection and stops there", () => {
  const { device } = fakeDevice([]);
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "found", devices: [device] },
    { type: "select", device },
    { type: "back" },
  ]);
  assert.deepEqual([state.step, state.device], ["discover", null]);
  // Device selection is the first step now, so there is nowhere further back.
  state = run([{ type: "back" }], state);
  assert.equal(state.step, "discover");
});

test("provisioning mints a token, sends the payload, then the Wi-Fi credentials", async () => {
  const { device, sent } = fakeDevice([]);
  const stages: string[] = [];
  const grant: ClaimTokenGrant = {
    claim_token: "single-use",
    expires_at: new Date(Date.now() + 600_000).toISOString(),
    server_url: "https://mirror.example",
  };
  let mints = 0;
  await provisionDevice({
    device,
    ssid: "Home",
    passphrase: "secret",
    mint: async () => {
      mints += 1;
      return grant;
    },
    onStage: (stage) => stages.push(stage),
  });
  assert.equal(mints, 1);
  assert.deepEqual(stages, ["token", "payload", "credentials"]);
  assert.deepEqual(sent.payload, {
    server_url: "https://mirror.example",
    claim_token: "single-use",
  });
  assert.deepEqual(sent.wifi, ["Home", "secret"]);
});

test("an expired claim token is replaced automatically", async () => {
  const { device, sent } = fakeDevice([]);
  const grants: ClaimTokenGrant[] = [
    {
      claim_token: "stale",
      expires_at: "2026-09-15T12:00:00Z",
      server_url: "https://mirror.example",
    },
    {
      claim_token: "fresh",
      expires_at: "2026-09-15T12:30:00Z",
      server_url: "https://mirror.example",
    },
  ];
  await provisionDevice({
    device,
    ssid: "Home",
    passphrase: "secret",
    now: () => Date.parse("2026-09-15T12:05:00Z"),
    mint: async () => grants.shift()!,
  });
  assert.equal(sent.payload?.claim_token, "fresh");
});

test("two expired tokens in a row stop the flow rather than pairing blindly", async () => {
  const { device } = fakeDevice([]);
  await assert.rejects(
    () =>
      provisionDevice({
        device,
        ssid: "Home",
        passphrase: "secret",
        now: () => Date.parse("2026-09-15T12:05:00Z"),
        mint: async () => ({
          claim_token: "stale",
          expires_at: "2026-09-15T12:00:00Z",
          server_url: "https://mirror.example",
        }),
      }),
    /date and time/,
  );
});

test("confirmation polling returns the first terminal reply", async () => {
  const { device, sent } = fakeDevice([
    { status: "awaiting_confirm" },
    { status: "awaiting_confirm" },
    { status: "claimed", device_name: "Hallway" },
  ]);
  let now = 0;
  const result = await awaitConfirmation({
    device,
    now: () => now,
    wait: async (ms) => void (now += ms),
  });
  assert.deepEqual(result, { status: "claimed", device_name: "Hallway" });
  assert.equal(sent.reads, 3);
});

test("polling gives up when the window closes", async () => {
  const { device } = fakeDevice(
    Array.from(
      { length: 100 },
      () => ({ status: "awaiting_confirm" }) as const,
    ),
  );
  let now = 0;
  assert.equal(
    await awaitConfirmation({
      device,
      timeoutMs: 5_000,
      intervalMs: 1_000,
      now: () => now,
      wait: async (ms) => void (now += ms),
    }),
    null,
  );
});

test("a single dropped read is tolerated but a persistent one surfaces", async () => {
  let reads = 0;
  const { device } = fakeDevice([], {
    async readResult() {
      reads += 1;
      if (reads === 1) throw new Error("socket closed");
      return { status: "claimed", device_name: "Hallway" };
    },
  });
  let now = 0;
  assert.deepEqual(
    await awaitConfirmation({
      device,
      now: () => now,
      wait: async (ms) => void (now += ms),
    }),
    { status: "claimed", device_name: "Hallway" },
  );
  const broken = fakeDevice([], {
    readResult: async () => {
      throw new Error("The network connection was lost");
    },
  });
  now = 0;
  await assert.rejects(
    () =>
      awaitConfirmation({
        device: broken.device,
        timeoutMs: 3_000,
        intervalMs: 1_000,
        now: () => now,
        wait: async (ms) => void (now += ms),
      }),
    /connection was lost/,
  );
});

test("polling stops quietly when the screen goes away", async () => {
  const { device } = fakeDevice([{ status: "awaiting_confirm" }]);
  assert.equal(
    await awaitConfirmation({ device, signal: { aborted: true } }),
    null,
  );
});

test("errors are turned into words a person can act on", () => {
  assert.match(
    stepErrorMessage(new ApiError(401, "expired"), "sending"),
    /Sign in again/,
  );
  assert.equal(
    stepErrorMessage(new ApiError(500, "The server could not"), "sending"),
    "The server could not",
  );
  assert.match(
    stepErrorMessage(new Error("Request aborted"), "discover"),
    /No camera answered/,
  );
  assert.match(
    stepErrorMessage(new Error("could not connect to host"), "wifi"),
    /Bluetooth is on/,
  );
  assert.match(
    stepErrorMessage(new Error("Request timed out"), "confirm"),
    /Move closer to it/,
  );
});
