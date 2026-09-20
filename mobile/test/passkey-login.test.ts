import { test } from "node:test";
import assert from "node:assert/strict";
import { MirrorApi } from "@daily-mirror/api";
import {
  loginWithPasskey,
  passkeyRequest,
  passkeyErrorMessage,
  PASSKEY_DOMAIN,
} from "../src/passkey-login";

test("native passkey login forwards the challenge and finishes with a normalized credential", async () => {
  const original = globalThis.fetch;
  const calls: { url: string; body: any; options?: RequestInit }[] = [];
  const session = {
    token: "test-session",
    expires_in_seconds: 3600,
    user: { id: "user", username: "drew", display_name: "Drew" },
  };
  const publicKey = {
    challenge: "Y2hhbGxlbmdl",
    rpId: PASSKEY_DOMAIN,
    allowCredentials: [],
  };
  globalThis.fetch = (async (url, options) => {
    calls.push({
      url: String(url),
      body: JSON.parse(String(options?.body)),
      options,
    });
    return Response.json(
      calls.length === 1
        ? { ceremony_id: "single-use", options: { publicKey } }
        : session,
    );
  }) as typeof fetch;
  try {
    const result = await loginWithPasskey(
      new MirrorApi(`https://${PASSKEY_DOMAIN}`),
      " drew ",
      async (request) => {
        assert.deepEqual(request, publicKey);
        return {
          id: "credential",
          response: {
            authenticatorData: "auth",
            clientDataJSON: "client",
            signature: "sig",
          },
        };
      },
    );
    assert.deepEqual(result, session);
    assert.equal(calls[0].body.username, "drew");
    assert.equal(calls[1].body.ceremony_id, "single-use");
    assert.equal(calls[1].body.credential.rawId, "credential");
    assert.equal(calls[1].body.credential.type, "public-key");
    assert.equal(calls[1].options?.credentials, "omit");
    assert.ok(calls[1].url.endsWith("/native/passkey/finish"));
  } finally {
    globalThis.fetch = original;
  }
});

test("cancelling the native prompt does not attempt session creation", async () => {
  const original = globalThis.fetch;
  let calls = 0;
  globalThis.fetch = (async () => {
    calls++;
    return Response.json({
      ceremony_id: "one",
      options: { publicKey: { challenge: "abc", rpId: PASSKEY_DOMAIN } },
    });
  }) as typeof fetch;
  try {
    const cancelled = { error: "UserCancelled" };
    await assert.rejects(
      loginWithPasskey(
        new MirrorApi(`https://${PASSKEY_DOMAIN}`),
        "drew",
        async () => {
          throw cancelled;
        },
      ),
      (error) => error === cancelled,
    );
    assert.equal(calls, 1);
    assert.equal(passkeyErrorMessage(cancelled), "");
  } finally {
    globalThis.fetch = original;
  }
});

test("unassociated domains and invalid challenges never reach the native prompt", async () => {
  for (const value of [
    null,
    {},
    { publicKey: { challenge: "abc", rpId: "other.example" } },
    { publicKey: { rpId: PASSKEY_DOMAIN } },
  ])
    assert.throws(() => passkeyRequest(value));
  await assert.rejects(
    loginWithPasskey(
      new MirrorApi("https://other.example"),
      "drew",
      async () => {
        throw new Error("Must not prompt");
      },
    ),
    /not configured/,
  );
});

test("an empty username asks for a discoverable ceremony", async () => {
  const original = globalThis.fetch;
  const calls: { url: string; body: any }[] = [];
  const session = {
    token: "test-session",
    expires_in_seconds: 3600,
    user: { id: "user", username: "drew", display_name: "Drew" },
  };
  globalThis.fetch = (async (url, options) => {
    calls.push({ url: String(url), body: JSON.parse(String(options?.body)) });
    return Response.json(
      calls.length === 1
        ? {
            ceremony_id: "discoverable",
            // No allowCredentials: iOS picks the account itself.
            options: {
              publicKey: {
                challenge: "Y2hhbGxlbmdl",
                rpId: PASSKEY_DOMAIN,
                allowCredentials: [],
              },
            },
          }
        : session,
    );
  }) as typeof fetch;
  try {
    const result = await loginWithPasskey(
      new MirrorApi(`https://${PASSKEY_DOMAIN}`),
      "   ",
      async (request) => {
        assert.deepEqual(request.allowCredentials, []);
        return {
          id: "picked",
          response: {
            authenticatorData: "auth",
            clientDataJSON: "client",
            signature: "sig",
            userHandle: "handle",
          },
        };
      },
    );
    assert.deepEqual(result, session);
    // The username key is absent entirely, not sent as an empty string.
    assert.deepEqual(calls[0].body, {});
    assert.equal(calls[1].body.ceremony_id, "discoverable");
    assert.equal(calls[1].body.credential.rawId, "picked");
  } finally {
    globalThis.fetch = original;
  }
});
