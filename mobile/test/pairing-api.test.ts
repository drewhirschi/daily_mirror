import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError, MirrorApi } from "@daily-mirror/api";
import { listDevices, mintClaimToken } from "../src/pairing/api";
import {
  claimTokenExpired,
  parseProvisioningResult,
} from "../src/pairing/contract";

const device = {
  device_id: "mirror-1",
  device_name: "Hallway",
  hardware: "esp32-p4-imx519",
  firmware_version: "0.2.0",
  claimed_at: "2026-09-01T10:00:00Z",
  last_seen_at: null,
};

function stubFetch(handler: (url: string, init?: RequestInit) => Response) {
  const original = globalThis.fetch;
  const calls: [string, RequestInit | undefined][] = [];
  globalThis.fetch = (async (url: unknown, init?: RequestInit) => {
    calls.push([String(url), init]);
    return handler(String(url), init);
  }) as typeof fetch;
  return { calls, restore: () => void (globalThis.fetch = original) };
}

test("the device list is fetched with bearer auth and validated", async () => {
  const stub = stubFetch(() => Response.json({ devices: [device] }));
  try {
    const api = new MirrorApi("https://mirror.example", "session-secret");
    assert.deepEqual(await listDevices(api), [device]);
    assert.equal(stub.calls[0][0], "https://mirror.example/api/devices");
    const headers = stub.calls[0][1]?.headers as Record<string, string>;
    assert.equal(headers.Authorization, "Bearer session-secret");
    assert.equal(stub.calls[0][1]?.credentials, "omit");
  } finally {
    stub.restore();
  }
});

test("a bare array of devices is accepted and a malformed row is rejected", async () => {
  let body: unknown = [device];
  const stub = stubFetch(() => Response.json(body));
  try {
    const api = new MirrorApi("https://mirror.example", "token");
    assert.deepEqual(await listDevices(api), [device]);
    body = { devices: [{ ...device, firmware_version: 3 }] };
    await assert.rejects(() => listDevices(api), /unexpected device list/);
  } finally {
    stub.restore();
  }
});

test("a server without the pairing routes reports a plain 404 message", async () => {
  const stub = stubFetch(() => new Response(null, { status: 404 }));
  try {
    const api = new MirrorApi("https://mirror.example", "token");
    await assert.rejects(
      () => listDevices(api),
      (error: unknown) => {
        assert.ok(error instanceof ApiError);
        assert.equal(error.status, 404);
        assert.match(error.message, /device pairing update/);
        return true;
      },
    );
  } finally {
    stub.restore();
  }
});

test("minting a claim token posts to the claim-tokens route and checks the grant", async () => {
  let grant: unknown = {
    claim_token: "single-use",
    expires_at: "2026-09-15T12:10:00Z",
    server_url: "https://mirror.example",
  };
  const stub = stubFetch(() => Response.json(grant));
  try {
    const api = new MirrorApi("https://mirror.example", "token");
    const result = await mintClaimToken(api);
    assert.equal(result.claim_token, "single-use");
    assert.equal(
      stub.calls[0][0],
      "https://mirror.example/api/devices/claim-tokens",
    );
    assert.equal(stub.calls[0][1]?.method, "POST");
    grant = { claim_token: "", expires_at: "", server_url: "" };
    await assert.rejects(() => mintClaimToken(api), /unexpected claim token/);
    grant = {
      claim_token: "t",
      expires_at: "2026-09-15T12:10:00Z",
      server_url: "ftp://mirror.example",
    };
    await assert.rejects(() => mintClaimToken(api), /unexpected claim token/);
  } finally {
    stub.restore();
  }
});

test("claim token expiry is decided from the grant timestamp", () => {
  const at = Date.parse("2026-09-15T12:10:00Z");
  assert.equal(
    claimTokenExpired({ expires_at: "2026-09-15T12:10:00Z" }, at - 1),
    false,
  );
  assert.equal(
    claimTokenExpired({ expires_at: "2026-09-15T12:10:00Z" }, at),
    true,
  );
  assert.equal(claimTokenExpired({ expires_at: "soon" }, at), true);
});

test("provisioning replies are parsed and anything else is refused", () => {
  assert.deepEqual(parseProvisioningResult('{"status":"awaiting_confirm"}'), {
    status: "awaiting_confirm",
  });
  assert.deepEqual(
    parseProvisioningResult('{"status":"claimed","device_name":"Hallway"}'),
    { status: "claimed", device_name: "Hallway" },
  );
  assert.deepEqual(
    parseProvisioningResult('{"status":"failed","reason":"wifi_auth"}'),
    { status: "failed", reason: "wifi_auth" },
  );
  assert.throws(
    () => parseProvisioningResult("not json"),
    /did not understand/,
  );
  assert.throws(
    () => parseProvisioningResult('{"status":"claimed"}'),
    /did not understand/,
  );
});
