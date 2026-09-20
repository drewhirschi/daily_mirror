import { ApiError } from "@daily-mirror/api";
import {
  CONFIRM_WINDOW_MS,
  claimTokenExpired,
  type ClaimTokenGrant,
  type ProvisioningResult,
} from "./contract";
import type { PairingDevice, WifiNetwork } from "./provisioning";

/**
 * The "Add a camera" flow as a pure state machine. Every screen is one step;
 * every asynchronous result arrives as an event. Keeping it free of React and
 * of the Espressif library is what makes it testable in `mobile/test`.
 *
 * Over BLE the flow opens straight on `discover` ("Select your camera"). The
 * SoftAP fallback inserts `join` in front of it, because iOS needs the phone on
 * the camera's own network before a scan can see anything.
 */

export type Step =
  "join" | "discover" | "wifi" | "sending" | "confirm" | "success";

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
  | { type: "begin"; needsManualJoin: boolean }
  | { type: "joined" }
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
  step: "discover",
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
    case "begin":
      return {
        ...state,
        ...clear,
        step: event.needsManualJoin ? "join" : "discover",
        busy: !event.needsManualJoin,
      };
    case "joined":
      return { ...state, ...clear, step: "discover", busy: true };
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
          retryStep: "sending",
        };
      return state;
    case "timeout":
      return {
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
        : state.retryStep === "wifi"
          ? { ...state, ...clear, step: "wifi", busy: false }
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

/** Anything thrown during the flow, in words a person can act on. */
export function stepErrorMessage(error: unknown, step: Step): string {
  if (error instanceof ApiError)
    return error.status === 401
      ? "Your session has expired. Sign in again, then add the camera."
      : error.message;
  const text = error instanceof Error ? error.message : String(error);
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
