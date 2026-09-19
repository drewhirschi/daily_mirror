import { ApiError, type MirrorApi } from "@daily-mirror/api";
import type { ClaimTokenGrant, DeviceSummary } from "./contract";

/**
 * Hand-typed calls for the two pairing routes. `MirrorApi` is generated from
 * the server's OpenAPI document, which does not describe `/api/devices` yet;
 * move these onto `MirrorApi` once `npm run api:generate` emits them.
 */

type DeviceList = { devices: DeviceSummary[] };

async function request<T>(
  api: MirrorApi,
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (init.signal?.aborted) abort();
  init.signal?.addEventListener("abort", abort);
  const timer = setTimeout(abort, 30_000);
  try {
    const response = await fetch(`${api.origin}${path}`, {
      ...init,
      signal: controller.signal,
      credentials: "omit",
      headers: {
        Accept: "application/json",
        ...api.headers,
        ...(init.body ? { "Content-Type": "application/json" } : {}),
      },
    });
    if (!response.ok) {
      throw new ApiError(
        response.status,
        response.status === 401
          ? "Your session has expired. Please sign in again."
          : response.status === 404
            ? "This server needs the device pairing update first."
            : `The server could not complete this request (${response.status}).`,
      );
    }
    return response.status === 204
      ? (undefined as T)
      : ((await response.json()) as T);
  } finally {
    clearTimeout(timer);
    init.signal?.removeEventListener("abort", abort);
  }
}

function isDeviceSummary(value: unknown): value is DeviceSummary {
  const row = value as Partial<DeviceSummary> | null;
  return (
    !!row &&
    typeof row === "object" &&
    typeof row.device_id === "string" &&
    typeof row.device_name === "string" &&
    typeof row.hardware === "string" &&
    typeof row.firmware_version === "string" &&
    typeof row.claimed_at === "string" &&
    (row.last_seen_at === null || typeof row.last_seen_at === "string")
  );
}

/** `GET /api/devices` — the signed-in household's claimed mirrors. */
export async function listDevices(
  api: MirrorApi,
  signal?: AbortSignal,
): Promise<DeviceSummary[]> {
  const body = await request<DeviceList | DeviceSummary[]>(
    api,
    "/api/devices",
    { signal },
  );
  const devices = Array.isArray(body) ? body : body?.devices;
  if (!Array.isArray(devices) || !devices.every(isDeviceSummary))
    throw new Error("The server returned an unexpected device list.");
  return devices;
}

/**
 * `POST /api/devices/claim-tokens` — a single-use token bound to the caller's
 * household, together with the origin the mirror should talk to. The token is
 * short-lived, so mint it immediately before handing it to a device.
 */
export async function mintClaimToken(
  api: MirrorApi,
  signal?: AbortSignal,
): Promise<ClaimTokenGrant> {
  const grant = await request<ClaimTokenGrant>(
    api,
    "/api/devices/claim-tokens",
    { method: "POST", body: "{}", signal },
  );
  if (
    !grant ||
    typeof grant.claim_token !== "string" ||
    !grant.claim_token ||
    typeof grant.expires_at !== "string" ||
    typeof grant.server_url !== "string"
  )
    throw new Error("The server returned an unexpected claim token.");
  // The device is told where to redeem the token; never let the server point
  // it at an origin that is not an https (or dev http) URL.
  const url = new URL(grant.server_url);
  if (url.protocol !== "https:" && url.protocol !== "http:")
    throw new Error("The server returned an unexpected claim token.");
  return grant;
}
