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

/** The device waits this long for the confirming button press. */
export const CONFIRM_WINDOW_MS = 30_000;

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
