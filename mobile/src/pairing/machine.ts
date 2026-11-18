import { ApiError } from "@daily-mirror/api";
import {
  CONFIRM_WINDOW_MS,
  SOFTAP_JOIN_TIMEOUT_MS,
  claimTokenExpired,
  deviceIdSuffix,
  type ClaimTokenGrant,
  type DeviceSummary,
  type ProvisioningResult,
} from "./contract";
import type { PairingDevice, WifiNetwork } from "./provisioning";

/**
 * The "Add a camera" flow as a pure state machine. Every screen is one step;
 * every asynchronous result arrives as an event. Keeping it free of React and
 * of the Espressif library is what makes it testable in `mobile/test`.
 *
 * The flow opens on `choose` ("Which camera do you have?"). The Bluetooth
 * camera goes on to `discover` ("Select your camera"). The
 * SoftAP fallback inserts `join` in front of it, because iOS needs the phone on
 * the camera's own network before a scan can see anything.
 *
 * A camera with no Bluetooth (the ESP32-P4) goes `prepare` -> `join` -> `wifi`
 * -> `sending` -> `confirm`. `prepare` runs on the home Wi-Fi: it gets the
 * pairing code from the server and waits for it, because once the phone is on
 * the camera's network there is no internet to get one from. After the send,
 * the camera closes its network as soon as it is on the home Wi-Fi, the phone
 * falls back to the home Wi-Fi by itself, and `confirm` asks the server, not
 * the camera, whether the camera has been claimed.
 */

export type Step =
  | "choose"
  | "prepare"
  | "join"
  | "discover"
  | "wifi"
  | "sending"
  | "confirm"
  | "success";

export type SendingStage = "token" | "payload" | "credentials";

export type PairingState = {
  step: Step;
  /** Devices found by the last scan. */
  found: PairingDevice[];
  device: PairingDevice | null;
  networks: WifiNetwork[];
  ssid: string;
  /** True while the flow is waiting on the device, the server or a scan. */
  busy: boolean;
  stage: SendingStage;
  /** Plain-language problem shown on the current step, if any. */
  error: string;
  /**
   * The raw text of whatever was thrown, kept beside the plain-language
   * message. Shown small and grey under the error: this build is a debugging
   * tool, and "could not reach the camera" hides the one line that says why.
   */
  detail: string;
  /** Which step "Try again" returns to. */
  retryStep: Step | null;
  deviceName: string;
  /** Epoch ms when the camera's 30 s confirmation window opened. */
  confirmDeadline: number;
  attempt: number;
};

export type PairingEvent =
  /** Back to "Which camera do you have?". */
  | { type: "choose" }
  /** The Bluetooth camera was chosen. */
  | { type: "begin"; needsManualJoin: boolean }
  | { type: "joined" }
  /** The camera has no Bluetooth; set it up over its own Wi-Fi instead. */
  | { type: "useWifi" }
  /** The pairing code for a camera without Bluetooth is in hand. */
  | { type: "prepared" }
  | { type: "scan" }
  | { type: "found"; devices: PairingDevice[] }
  | { type: "select"; device: PairingDevice }
  | { type: "networks"; networks: WifiNetwork[] }
  | { type: "ssid"; ssid: string }
  | { type: "send" }
  | { type: "stage"; stage: SendingStage }
  | { type: "awaiting"; now?: number }
  | { type: "result"; result: ProvisioningResult }
  | { type: "timeout" }
  | { type: "failed"; message: string; detail?: string; retryStep: Step }
  | { type: "retry" }
  | { type: "back" };

export const initialPairingState: PairingState = {
  step: "choose",
  found: [],
  device: null,
  networks: [],
  ssid: "",
  busy: false,
  stage: "token",
  error: "",
  detail: "",
  retryStep: null,
  deviceName: "",
  confirmDeadline: 0,
  attempt: 0,
};

const clear = { error: "", detail: "", retryStep: null } as const;

export function pairingReducer(
  state: PairingState,
  event: PairingEvent,
): PairingState {
  switch (event.type) {
    case "choose":
      return {
        ...state,
        ...clear,
        step: "choose",
        busy: false,
        device: null,
        found: [],
      };
    case "begin":
      return {
        ...state,
        ...clear,
        step: event.needsManualJoin ? "join" : "discover",
        busy: !event.needsManualJoin,
      };
    case "joined":
      return { ...state, ...clear, step: "discover", busy: true };
    case "useWifi":
      return { ...state, ...clear, step: "prepare", busy: true, device: null };
    case "prepared":
      return { ...state, ...clear, step: "join", busy: false };
    case "scan":
      return { ...state, ...clear, step: "discover", busy: true, found: [] };
    case "found":
      return {
        ...state,
        ...clear,
        step: "discover",
        busy: false,
        found: event.devices,
      };
    case "select":
      return {
        ...state,
        ...clear,
        step: "wifi",
        device: event.device,
        networks: [],
        busy: true,
      };
    case "networks":
      return { ...state, busy: false, networks: event.networks };
    case "ssid":
      return { ...state, ssid: event.ssid };
    case "send":
      return {
        ...state,
        ...clear,
        step: "sending",
        busy: true,
        stage: "token",
        attempt: state.attempt + 1,
      };
    case "stage":
      return { ...state, stage: event.stage };
    case "awaiting":
      return {
        ...state,
        ...clear,
        step: "confirm",
        busy: true,
        confirmDeadline: (event.now ?? Date.now()) + CONFIRM_WINDOW_MS,
      };
    case "result":
      if (event.result.status === "claimed")
        return {
          ...state,
          ...clear,
          step: "success",
          busy: false,
          deviceName: event.result.device_name,
        };
      if (event.result.status === "failed")
        return {
          ...state,
          step: "confirm",
          busy: false,
          error: failureMessage(event.result.reason),
          detail: event.result.reason,
          // Over SoftAP a reported failure means the camera never left its
          // own network (a wrong password, say): fix the details and resend.
          retryStep: state.device?.transport === "softap" ? "wifi" : "sending",
        };
      return state;
    case "timeout":
      return state.device?.transport === "softap"
        ? {
            ...state,
            step: "confirm",
            busy: false,
            error:
              "The camera did not show up in your household. The button may not have been pressed in time. Hold the camera's button for five seconds to reopen pairing, check this phone is back on your home Wi-Fi, then tap Try again.",
            detail: "",
            retryStep: "prepare",
          }
        : {
            ...state,
            step: "confirm",
            busy: false,
            error:
              "The camera did not get its button press in time. Try again and press the button once while the ring pulses amber.",
            retryStep: "sending",
          };
    case "failed":
      return {
        ...state,
        busy: false,
        step: event.retryStep,
        error: event.message,
        detail: event.detail ?? "",
        retryStep: event.retryStep,
      };
    case "retry":
      return state.retryStep === "sending"
        ? {
            ...state,
            ...clear,
            step: "sending",
            busy: true,
            stage: "token",
            attempt: state.attempt + 1,
          }
        : state.retryStep === "prepare"
          ? { ...state, ...clear, step: "prepare", busy: true, device: null }
          : state.retryStep === "wifi"
            ? {
                ...state,
                ...clear,
                step: "wifi",
                // A SoftAP camera that reported a failed join went back to
                // pairing; reconnect to it before resending.
                busy: state.device?.transport === "softap",
              }
            : state.retryStep === "join"
              ? { ...state, ...clear, step: "join", busy: false }
              : { ...state, ...clear, step: "discover", busy: true, found: [] };
    case "back":
      return state.step === "wifi"
        ? { ...state, ...clear, step: "discover", busy: false, device: null }
        : state;
  }
}

/** A camera-reported failure, in words a person can act on. */
export function failureMessage(reason: string): string {
  if (/wifi|wi-fi|ssid|password|passphrase|auth/i.test(reason))
    return "The camera could not join that Wi-Fi network. Check the name and password and try again.";
  if (/expire/i.test(reason))
    return "The pairing code ran out. Try again and the app will get a fresh one.";
  if (/claim|household|server|http/i.test(reason))
    return "The camera reached your Wi-Fi but could not finish signing in to Daily Mirror. Try again.";
  return "The camera could not finish pairing. Try again.";
}

/**
 * The unedited text of whatever was thrown, for the debug line under the
 * message. The plain-language wording above it deliberately loses detail; on
 * real hardware that detail is the whole diagnosis.
 */
export function stepErrorDetail(error: unknown, step: Step): string {
  const text = error instanceof Error ? error.message : String(error);
  /* A server failure and a Bluetooth failure read almost the same once they
   * have been turned into plain language, so say which one this was. */
  const where = error instanceof ApiError ? `server ${error.status}` : step;
  return text ? `${where}: ${text}` : "";
}

/**
 * Thrown instead of minting a pairing code while the phone is on a camera's
 * own network, where there is no internet to mint one with.
 */
export class HomeWifiNeeded extends Error {
  constructor() {
    super(
      "The pairing code ran out. Switch this phone back to your home Wi-Fi, then tap Try again.",
    );
    this.name = "HomeWifiNeeded";
  }
}

/**
 * A pairing code held for a camera without Bluetooth, usable only while it
 * will outlive the rest of the flow: the camera redeems it up to
 * SOFTAP_CLAIM_BUDGET_MS after the send.
 */
export function heldGrantUsable(
  grant: ClaimTokenGrant | null,
  now = Date.now(),
): grant is ClaimTokenGrant {
  return !!grant && !claimTokenExpired(grant, now + SOFTAP_CLAIM_BUDGET_MS);
}

/** Anything thrown during the flow, in words a person can act on. */
export function stepErrorMessage(error: unknown, step: Step): string {
  if (error instanceof HomeWifiNeeded) return error.message;
  if (error instanceof ApiError)
    return error.status === 401
      ? "Your session has expired. Sign in again, then add the camera."
      : error.message;
  const text = error instanceof Error ? error.message : String(error);
  if (step === "prepare")
    return "Could not get a pairing code from Daily Mirror. Check this phone is on your home Wi-Fi with internet, then try again.";
  if (/abort|timeout|timed out/i.test(text))
    return step === "discover"
      ? "No camera answered. Check that the ring is chasing amber and that Bluetooth is on, then scan again."
      : "The camera stopped answering. Move closer to it and try again.";
  if (/network|connect|reach|socket|session/i.test(text))
    return "The app could not reach the camera. Move closer to it, check Bluetooth is on, then try again.";
  return text || "Something went wrong. Try again.";
}

export type ProvisionDeps = {
  device: PairingDevice;
  ssid: string;
  passphrase: string;
  /** Mints a claim token; called again automatically if one has expired. */
  mint: () => Promise<ClaimTokenGrant>;
  onStage?: (stage: SendingStage) => void;
  now?: () => number;
};

/**
 * Hand a camera its claim token and Wi-Fi credentials. The payload goes first:
 * some firmware closes the provisioning session once Wi-Fi succeeds, and the
 * custom endpoint needs that session.
 */
export async function provisionDevice({
  device,
  ssid,
  passphrase,
  mint,
  onStage,
  now = Date.now,
}: ProvisionDeps): Promise<void> {
  onStage?.("token");
  let grant = await mint();
  if (claimTokenExpired(grant, now())) grant = await mint();
  if (claimTokenExpired(grant, now()))
    throw new Error(
      "The pairing code expired immediately. Check this phone's date and time.",
    );
  onStage?.("payload");
  await device.sendPayload({
    server_url: grant.server_url,
    claim_token: grant.claim_token,
  });
  onStage?.("credentials");
  await device.sendWifiCredentials(ssid, passphrase);
}

export type ConfirmDeps = {
  device: PairingDevice;
  /** Overall budget; the camera's own window is 30 s. */
  timeoutMs?: number;
  intervalMs?: number;
  wait?: (ms: number) => Promise<void>;
  now?: () => number;
  signal?: { aborted: boolean };
};

/**
 * Poll the custom endpoint while the camera waits for its button press.
 * Resolves with the terminal result, or `null` when the window closed.
 */
export async function awaitConfirmation({
  device,
  timeoutMs = CONFIRM_WINDOW_MS + 10_000,
  intervalMs = 1_500,
  wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now = Date.now,
  signal,
}: ConfirmDeps): Promise<ProvisioningResult | null> {
  const deadline = now() + timeoutMs;
  let lastError: unknown = null;
  while (now() < deadline && !signal?.aborted) {
    try {
      const result = await device.readResult();
      if (result.status !== "awaiting_confirm") return result;
      lastError = null;
    } catch (error) {
      // A single dropped read is normal while the camera switches networks.
      lastError = error;
    }
    if (now() + intervalMs >= deadline) break;
    await wait(intervalMs);
  }
  if (signal?.aborted) return null;
  if (lastError) throw lastError;
  return null;
}

/**
 * How long after a SoftAP send the camera may still redeem its code: it has
 * SOFTAP_JOIN_TIMEOUT_MS to join the home Wi-Fi, then a fresh 30 s button
 * window, then the claim itself.
 */
export const SOFTAP_CLAIM_BUDGET_MS =
  SOFTAP_JOIN_TIMEOUT_MS + CONFIRM_WINDOW_MS + 20_000;

/**
 * How long the confirm step polls the server: the join, the 30 s button
 * window, then the claim, with room for the phone to rejoin home Wi-Fi.
 */
export const SOFTAP_CONFIRM_POLL_MS = SOFTAP_CLAIM_BUDGET_MS + 10_000;

/** Which claimed_at each device had before this pairing began. */
export type DeviceBaseline = Map<string, string>;

export function deviceBaseline(devices: DeviceSummary[]): DeviceBaseline {
  return new Map(devices.map((d) => [d.device_id, d.claimed_at]));
}

/**
 * The device this pairing claimed: one that is new to the household, or one
 * whose claim time moved (a re-pair keeps its id and refreshes claimed_at).
 * Without a baseline, anything claimed since `since` (less a minute of clock
 * skew) counts.
 */
export function newlyClaimed(
  devices: DeviceSummary[],
  baseline: DeviceBaseline | null,
  since: number,
  /** Hex tail of the camera's name; preferred when several match. */
  idSuffix: string | null = null,
): DeviceSummary | null {
  const fresh = devices.filter((d) =>
    baseline
      ? baseline.get(d.device_id) !== d.claimed_at
      : Date.parse(d.claimed_at) >= since - 60_000,
  );
  return (
    (idSuffix
      ? fresh.find((d) => d.device_id.toLowerCase().endsWith(idSuffix))
      : undefined) ??
    fresh[0] ??
    null
  );
}

export type ServerClaimDeps = {
  /** `GET /api/devices`; fails while the phone is still on the camera's network. */
  listDevices: () => Promise<DeviceSummary[]>;
  baseline: DeviceBaseline | null;
  /** Epoch ms when the pairing code was minted. */
  since: number;
  /**
   * The camera, read while it is still reachable so a failed join (which keeps
   * its network up) is reported in its own words. Unreachable is normal.
   */
  device?: PairingDevice;
  timeoutMs?: number;
  intervalMs?: number;
  /**
   * How long the camera is read at all. A failed join is reported within
   * SOFTAP_JOIN_TIMEOUT_MS or so; after that the camera's network is gone.
   */
  deviceReadMs?: number;
  /** Bound on one camera read; the library can hang on a dead socket. */
  readTimeoutMs?: number;
  wait?: (ms: number) => Promise<void>;
  now?: () => number;
  signal?: { aborted: boolean };
};

/**
 * SoftAP confirmation, asked of the server over the home Wi-Fi: resolves
 * "claimed" once the camera has redeemed its code into the household,
 * "failed" if the camera reports a failure before it leaves its own network,
 * or `null` when the budget runs out.
 */
export async function awaitServerClaim({
  listDevices,
  baseline,
  since,
  device,
  timeoutMs = SOFTAP_CONFIRM_POLL_MS,
  intervalMs = 2_000,
  deviceReadMs = 15_000,
  readTimeoutMs = 3_000,
  wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  now = Date.now,
  signal,
}: ServerClaimDeps): Promise<ProvisioningResult | null> {
  const start = now();
  const deadline = start + timeoutMs;
  const suffix = device ? deviceIdSuffix(device.name) : null;
  let reading = false;
  let reported: ProvisioningResult | null = null;
  const readDevice = () => {
    if (!device || reading || now() - start >= deviceReadMs) return;
    reading = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    void Promise.race([
      device.readResult(),
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), readTimeoutMs);
      }),
    ])
      .then((result) => {
        if (result && result.status !== "awaiting_confirm") reported = result;
      })
      .catch(() => undefined)
      .finally(() => {
        clearTimeout(timer);
        reading = false;
      });
  };
  while (now() < deadline && !signal?.aborted) {
    readDevice();
    try {
      const claimed = newlyClaimed(
        await listDevices(),
        baseline,
        since,
        suffix,
      );
      if (claimed)
        return { status: "claimed", device_name: claimed.device_name };
    } catch {
      // No internet yet: the phone is still on, or just leaving, the camera's
      // network.
    }
    if (reported) return reported;
    if (now() + intervalMs >= deadline) break;
    await wait(intervalMs);
    if (reported) return reported;
  }
  return null;
}
