import { ApiError, type MirrorApi } from "@daily-mirror/api";
import type { PasskeyGetRequest, PasskeyGetResult } from "react-native-passkey";

// Match the Associated Domains entitlement in app.config.ts.
export const PASSKEY_DOMAIN = "daily-mirror-pearl.vercel.app";

export function passkeyRequest(options: unknown): PasskeyGetRequest {
  const request = (options as { publicKey?: PasskeyGetRequest } | null)
    ?.publicKey;
  if (
    !request ||
    typeof request.challenge !== "string" ||
    !request.challenge ||
    request.rpId !== PASSKEY_DOMAIN
  ) {
    throw new Error("The server returned an invalid passkey request.");
  }
  return request;
}

/** Keep the native prompt separate so the complete server exchange is testable. */
export async function loginWithPasskey(
  api: MirrorApi,
  username: string,
  authenticate: (request: PasskeyGetRequest) => Promise<PasskeyGetResult>,
) {
  if (api.origin !== `https://${PASSKEY_DOMAIN}`) {
    throw new Error(
      "Passkeys are not configured for this server. Use your password to sign in.",
    );
  }
  let start;
  try {
    start = await api.passkeyStart(username.trim());
  } catch (error) {
    if (error instanceof ApiError && error.status === 404)
      throw new Error("The server needs the passkey login update first.");
    if (error instanceof ApiError && error.status === 401)
      throw new Error(
        "No passkey is available for this account. Check your username or sign in with your password.",
      );
    throw error;
  }
  const result = await authenticate(passkeyRequest(start.options));
  return api.passkeyFinish({
    ceremony_id: start.ceremony_id,
    credential: {
      ...result,
      rawId: result.rawId ?? result.id,
      type: result.type ?? "public-key",
      clientExtensionResults: result.clientExtensionResults ?? {},
    },
  });
}

export function passkeyErrorMessage(error: unknown): string {
  const code = (error as { error?: string } | null)?.error;
  if (code === "UserCancelled") return "";
  if (code === "BadConfiguration")
    return "Passkeys need the app’s website association to be enabled. You can still sign in with your password.";
  if (code === "NoCredentials" || code === "RequestFailed")
    return "No matching passkey was found. Use a saved passkey for this account or sign in with your password.";
  if (code === "Interrupted" || code === "TimedOut")
    return "Passkey sign-in was interrupted. Please try again.";
  return error instanceof Error
    ? error.message
    : "Could not sign in with your passkey. Please try again.";
}
