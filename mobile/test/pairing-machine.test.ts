import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError } from "@daily-mirror/api";
import {
  HomeWifiNeeded,
  SOFTAP_CLAIM_BUDGET_MS,
  awaitConfirmation,
  awaitServerClaim,
  deviceBaseline,
  heldGrantUsable,
  newlyClaimed,
  initialPairingState,
  pairingReducer,
  provisionDevice,
  stepErrorMessage,
  failureMessage,
  type PairingEvent,
  type PairingState,
} from "../src/pairing/machine";
import type { PairingDevice } from "../src/pairing/provisioning";
import { deviceIdSuffix } from "../src/pairing/contract";
import type {
  ClaimTokenGrant,
  DeviceSummary,
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
    transport: "ble",
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
  assert.equal(initialPairingState.step, "choose");
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

test("a camera without Bluetooth goes to the join step, and a failed join returns there", () => {
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "found", devices: [] },
    { type: "useWifi" },
  ]);
  // The pairing code is fetched on the home Wi-Fi before the join step.
  assert.deepEqual([state.step, state.busy], ["prepare", true]);
  state = run([{ type: "prepared" }], state);
  assert.equal(state.step, "join");
  assert.equal(state.busy, false);
  const { device } = fakeDevice([], { transport: "softap" });
  state = run(
    [
      { type: "select", device },
      { type: "failed", message: "no", retryStep: "join" },
      { type: "retry" },
    ],
    state,
  );
  assert.equal(state.step, "join");
  assert.equal(state.busy, false);
  assert.equal(state.error, "");
});

const row = (id: string, claimed_at: string): DeviceSummary => ({
  device_id: id,
  device_name: `Mirror ${id}`,
  hardware: "p4_imx519",
  firmware_version: "1",
  claimed_at,
  last_seen_at: null,
});

test("a failed pairing code stays on the home-Wi-Fi step and retries there", () => {
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "useWifi" },
    { type: "failed", message: "no code", retryStep: "prepare" },
  ]);
  assert.deepEqual(
    [state.step, state.busy, state.error, state.retryStep],
    ["prepare", false, "no code", "prepare"],
  );
  state = run([{ type: "retry" }], state);
  assert.deepEqual(
    [state.step, state.busy, state.error],
    ["prepare", true, ""],
  );
});

test("the SoftAP path confirms through the server and times out back to prepare", () => {
  const { device } = fakeDevice([], { transport: "softap" });
  let state = run([
    { type: "begin", needsManualJoin: false },
    { type: "useWifi" },
    { type: "prepared" },
    { type: "select", device },
    { type: "networks", networks: [] },
    { type: "ssid", ssid: "Home" },
    { type: "send" },
    { type: "awaiting", now: 0 },
  ]);
  assert.deepEqual([state.step, state.busy], ["confirm", true]);
  const timedOut = run([{ type: "timeout" }], state);
  assert.equal(timedOut.retryStep, "prepare");
  assert.match(timedOut.error, /five seconds to reopen pairing/);
  assert.equal(run([{ type: "retry" }], timedOut).step, "prepare");
  // A join failure the camera reported: fix the password and resend.
  const failed = run(
    [
      {
        type: "result",
        result: { status: "failed", reason: "wifi password was not accepted" },
      },
    ],
    state,
  );
  assert.equal(failed.retryStep, "wifi");
  assert.match(failed.error, /could not join that Wi-Fi/);
  // Retrying reconnects to the camera before the networks are shown again.
  assert.deepEqual(
    [
      run([{ type: "retry" }], failed).step,
      run([{ type: "retry" }], failed).busy,
    ],
    ["wifi", true],
  );
  state = run(
    [{ type: "result", result: { status: "claimed", device_name: "Hallway" } }],
    state,
  );
  assert.deepEqual([state.step, state.deviceName], ["success", "Hallway"]);
});

test("a held code is only used while it outlives the SoftAP flow", () => {
  const now = Date.parse("2026-10-17T12:00:00Z");
  const grant = (ms: number): ClaimTokenGrant => ({
    claim_token: "t",
    server_url: "https://example.test",
    expires_at: new Date(now + ms).toISOString(),
  });
  assert.equal(heldGrantUsable(null, now), false);
  assert.equal(heldGrantUsable(grant(10 * 60_000), now), true);
  assert.equal(
    heldGrantUsable(grant(SOFTAP_CLAIM_BUDGET_MS - 1_000), now),
    false,
  );
  assert.match(stepErrorMessage(new HomeWifiNeeded(), "sending"), /home Wi-Fi/);
});

test("a SoftAP send never mints: an expired held code asks for home Wi-Fi", async () => {
  const { device, sent } = fakeDevice([], { transport: "softap" });
  await assert.rejects(
    provisionDevice({
      device,
      ssid: "Home",
      passphrase: "pw",
      mint: async () => {
        throw new HomeWifiNeeded();
      },
    }),
    HomeWifiNeeded,
  );
  assert.equal(sent.payload, undefined);
});

test("newly claimed means a new device or a moved claim time", () => {
  const before = deviceBaseline([row("a", "2026-10-17T10:00:00Z")]);
  assert.equal(
    newlyClaimed([row("a", "2026-10-17T10:00:00Z")], before, 0),
    null,
  );
  assert.equal(
    newlyClaimed([row("a", "2026-10-17T12:00:00Z")], before, 0)?.device_id,
    "a",
  );
  assert.equal(
    newlyClaimed(
      [row("a", "2026-10-17T10:00:00Z"), row("b", "2026-10-17T12:00:00Z")],
      before,
      0,
    )?.device_id,
    "b",
  );
  const since = Date.parse("2026-10-17T12:00:00Z");
  assert.equal(
    newlyClaimed([row("a", "2026-10-17T10:00:00Z")], null, since),
    null,
  );
  assert.equal(
    newlyClaimed([row("a", "2026-10-17T11:59:30Z")], null, since)?.device_id,
    "a",
  );
});

test("server confirmation rides out a period with no internet, then sees the claim", async () => {
  let clock = 0;
  let calls = 0;
  const result = await awaitServerClaim({
    listDevices: async () => {
      calls += 1;
      if (calls < 3) throw new Error("Network request failed");
      return calls < 4 ? [] : [row("c", "2026-10-17T12:00:00Z")];
    },
    baseline: new Map(),
    since: 0,
    intervalMs: 1_000,
    wait: async (ms) => {
      clock += ms;
    },
    now: () => clock,
  });
  assert.deepEqual(result, { status: "claimed", device_name: "Mirror c" });
  assert.equal(calls, 4);
});

test("server confirmation reports a failure the camera still could say", async () => {
  let clock = 0;
  const { device } = fakeDevice(
    [{ status: "failed", reason: "wifi password was not accepted" }],
    { transport: "softap" },
  );
  const result = await awaitServerClaim({
    listDevices: async () => {
      throw new Error("offline");
    },
    baseline: null,
    since: 0,
    device,
    intervalMs: 1_000,
    wait: async (ms) => {
      clock += ms;
      await new Promise((resolve) => setImmediate(resolve));
    },
    now: () => clock,
  });
  assert.deepEqual(result, {
    status: "failed",
    reason: "wifi password was not accepted",
  });
});

test("server confirmation gives up with null when nothing is claimed", async () => {
  let clock = 0;
  const result = await awaitServerClaim({
    listDevices: async () => [],
    baseline: new Map(),
    since: 0,
    timeoutMs: 10_000,
    intervalMs: 2_000,
    wait: async (ms) => {
      clock += ms;
    },
    now: () => clock,
  });
  assert.equal(result, null);
});

test("the flow opens on the camera choice and can return to it", () => {
  assert.deepEqual(
    [initialPairingState.step, initialPairingState.busy],
    ["choose", false],
  );
  const ble = run([{ type: "begin", needsManualJoin: false }]);
  assert.deepEqual([ble.step, ble.busy], ["discover", true]);
  const wifi = run([{ type: "useWifi" }]);
  assert.deepEqual([wifi.step, wifi.busy], ["prepare", true]);
  const { device } = fakeDevice([], { transport: "softap" });
  const back = run(
    [{ type: "prepared" }, { type: "select", device }, { type: "choose" }],
    wifi,
  );
  assert.deepEqual(
    [back.step, back.busy, back.device],
    ["choose", false, null],
  );
});

test("deviceIdSuffix reads the hex tail of a camera name", () => {
  assert.equal(deviceIdSuffix("mirror-04807C"), "04807c");
  assert.equal(deviceIdSuffix("mirror-wifi"), null);
});

test("newly claimed prefers the device whose id ends in the name's suffix", () => {
  const since = Date.parse("2026-10-17T12:00:00Z");
  const rows = [
    row("aaaaaa111111", "2026-10-17T12:00:05Z"),
    row("8cfd4904807c", "2026-10-17T12:00:10Z"),
  ];
  assert.equal(
    newlyClaimed(rows, null, since, "04807c")?.device_id,
    "8cfd4904807c",
  );
  // A suffix never matches a device that was not claimed in this flow.
  const before = deviceBaseline([row("8cfd4904807c", "2026-10-17T10:00:00Z")]);
  assert.equal(
    newlyClaimed(
      [row("8cfd4904807c", "2026-10-17T10:00:00Z")],
      before,
      since,
      "04807c",
    ),
    null,
  );
});

test("server confirmation stops reading the camera after its read window", async () => {
  let clock = 0;
  const { device, sent } = fakeDevice([], {
    transport: "softap",
    name: "mirror-04807c",
  });
  const result = await awaitServerClaim({
    listDevices: async () => {
      throw new Error("offline");
    },
    baseline: null,
    since: 0,
    device,
    timeoutMs: 30_000,
    intervalMs: 2_000,
    deviceReadMs: 6_000,
    wait: async (ms) => {
      clock += ms;
      await new Promise((resolve) => setImmediate(resolve));
    },
    now: () => clock,
  });
  assert.equal(result, null);
  assert.equal(sent.reads, 3);
});

test("server confirmation matches the claimed camera by its id suffix", async () => {
  let clock = 0;
  const { device } = fakeDevice([], {
    transport: "softap",
    name: "mirror-04807c",
    readResult: async () => ({ status: "awaiting_confirm" }),
  });
  const result = await awaitServerClaim({
    listDevices: async () =>
      clock < 4_000
        ? []
        : [
            row("other", "2026-10-17T12:00:00Z"),
            row("8cfd4904807c", "2026-10-17T12:00:01Z"),
          ],
    baseline: new Map(),
    since: 0,
    device,
    intervalMs: 2_000,
    wait: async (ms) => {
      clock += ms;
    },
    now: () => clock,
  });
  assert.deepEqual(result, {
    status: "claimed",
    device_name: "Mirror 8cfd4904807c",
  });
});
