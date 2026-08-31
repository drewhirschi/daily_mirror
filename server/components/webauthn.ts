type JsonObject = Record<string, unknown>;

export function supportsPasskeys() {
  return typeof window !== "undefined" && "PublicKeyCredential" in window;
}

export async function createPasskey(options: JsonObject) {
  const publicKey = options.publicKey as JsonObject;
  const credential = await navigator.credentials.create({
    publicKey: {
      ...publicKey,
      challenge: decode(publicKey.challenge as string),
      user: {
        ...(publicKey.user as JsonObject),
        id: decode((publicKey.user as JsonObject).id as string),
      },
      excludeCredentials: ((publicKey.excludeCredentials as JsonObject[] | undefined) ?? []).map(
        (descriptor) => ({ ...descriptor, id: decode(descriptor.id as string) }),
      ),
    } as PublicKeyCredentialCreationOptions,
  });
  if (!(credential instanceof PublicKeyCredential)) throw new Error("No passkey was created.");
  const response = credential.response as AuthenticatorAttestationResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: encode(response.attestationObject),
      clientDataJSON: encode(response.clientDataJSON),
      transports: typeof response.getTransports === "function" ? response.getTransports() : [],
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  };
}

export async function getPasskey(options: JsonObject) {
  const publicKey = options.publicKey as JsonObject;
  const credential = await navigator.credentials.get({
    publicKey: {
      ...publicKey,
      challenge: decode(publicKey.challenge as string),
      allowCredentials: ((publicKey.allowCredentials as JsonObject[] | undefined) ?? []).map(
        (descriptor) => ({ ...descriptor, id: decode(descriptor.id as string) }),
      ),
    } as PublicKeyCredentialRequestOptions,
  });
  if (!(credential instanceof PublicKeyCredential)) throw new Error("No passkey was selected.");
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: encode(response.authenticatorData),
      clientDataJSON: encode(response.clientDataJSON),
      signature: encode(response.signature),
      userHandle: response.userHandle ? encode(response.userHandle) : null,
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  };
}

function decode(value: string) {
  const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
  return bytes.buffer;
}

function encode(value: ArrayBuffer) {
  const bytes = new Uint8Array(value);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}
