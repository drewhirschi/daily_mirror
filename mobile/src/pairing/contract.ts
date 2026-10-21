/**
 * Wire models for the pairing routes, hand-typed from
 * `crates/mirror-core/src/contract.rs`. The server routes are not in the
 * OpenAPI document yet; when they are, delete these declarations and take the
 * types from `@daily-mirror/api` instead.
 */

/** Response of `POST /api/devices/claim-tokens`. */
export type ClaimTokenGrant = {
  claim_token: string;
  /** RFC 3339 timestamp after which the token is rejected. */
  expires_at: string;
  server_url: string;
};

/** Row of `GET /api/devices`. */
export type DeviceSummary = {
  device_id: string;
  device_name: string;
  hardware: string;
  firmware_version: string;
  claimed_at: string;
  last_seen_at: string | null;
};

/** What the app sends over the local link to the `daily-mirror` endpoint. */
export type ProvisioningPayload = {
  server_url: string;
  claim_token: string;
};

/** What the device replies on the same endpoint. */
export type ProvisioningResult =
  | { status: "claimed"; device_name: string }
  | { status: "awaiting_confirm" }
  | { status: "failed"; reason: string };

/** Custom provisioning endpoint name agreed with the firmware. */
export const PROVISIONING_ENDPOINT = "daily-mirror";

/**
 * Device BLE / SoftAP name prefix. Cameras advertise as `mirror-<hex>`,
 * always lowercase, matching what the firmware registers.
 */
export const DEVICE_PREFIX = "mirror-";

/**
 * Protocomm security 2 (SRP6a) identity, agreed with the firmware's
 * CONFIG_MIRROR_PROV_USERNAME / CONFIG_MIRROR_PROV_POP. Neither is a secret,
 * but both must match the device byte for byte or the handshake fails.
 */
export const PROVISIONING_USERNAME = "mirror";
export const PROVISIONING_POP = "daily-mirror";

/**
 * The WPA2 password of a camera's own setup network, CONFIG_MIRROR_AP_PASSWORD
 * in the firmware. Like the proof of possession it is not a secret; it only
 * has to match. Used when a camera without Bluetooth (the ESP32-P4) is paired
 * over SoftAP: the person joins that network in Settings with it, so the app
 * shows it on the join step. The address the app then
 * talks to, 10.10.0.1, is the `WifiBaseUrl` Info.plist key in app.config.ts.
 */
export const SOFTAP_PASSWORD = "dailymirror";

/** The device waits this long for the confirming button press. */
export const CONFIRM_WINDOW_MS = 30_000;

/**
 * SoftAP only: how long the camera tries to join the home Wi-Fi after the
 * credentials before it reports the join as failed and keeps its own network
 * up. Once it has an address on the home Wi-Fi it closes its network and the
 * 30 s button window starts.
 */
export const SOFTAP_JOIN_TIMEOUT_MS = 20_000;

/**
 * The hex suffix of a camera's advertised name (`mirror-04807c` -> `04807c`),
 * which is the tail of its device_id. Null for a name without one.
 */
export function deviceIdSuffix(name: string): string | null {
  const match = /^mirror-([0-9a-f]{4,12})$/i.exec(name);
  return match ? match[1].toLowerCase() : null;
}

export function isProvisioningResult(
  value: unknown,
): value is ProvisioningResult {
  if (!value || typeof value !== "object") return false;
  const status = (value as { status?: unknown }).status;
  if (status === "awaiting_confirm") return true;
  if (status === "claimed")
    return typeof (value as { device_name?: unknown }).device_name === "string";
  if (status === "failed")
    return typeof (value as { reason?: unknown }).reason === "string";
  return false;
}

export function parseProvisioningResult(raw: string): ProvisioningResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("The mirror sent a reply this app did not understand.");
  }
  if (!isProvisioningResult(parsed))
    throw new Error("The mirror sent a reply this app did not understand.");
  return parsed;
}

export function claimTokenExpired(
  grant: Pick<ClaimTokenGrant, "expires_at">,
  now = Date.now(),
): boolean {
  const expires = Date.parse(grant.expires_at);
  // An unparseable timestamp is treated as expired so a fresh token is minted.
  return !Number.isFinite(expires) || expires <= now;
}
