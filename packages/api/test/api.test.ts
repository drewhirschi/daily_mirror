import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError, MirrorApi, mediaUrl, serverOrigin } from "../src/index";

test("server configuration requires HTTPS and rejects embedded credentials", () => {
  assert.equal(
    serverOrigin(" https://mirror.example/ "),
    "https://mirror.example",
  );
  assert.throws(() => serverOrigin("http://mirror.example"));
  assert.equal(
    serverOrigin("http://localhost:3000", true),
    "http://localhost:3000",
  );
  assert.throws(() => serverOrigin("https://name:secret@mirror.example"));
  assert.throws(() => serverOrigin("https://mirror.example/path"));
});
test("media credentials cannot be sent to a third party or an arbitrary server endpoint", () => {
  assert.equal(
    mediaUrl("https://mirror.example", "/api/photos/id/thumbnail?rev=2"),
    "https://mirror.example/api/photos/id/thumbnail?rev=2",
  );
  for (const path of [
    "https://other.example/api/photos/id",
    "//other.example/api/photos/id",
    "/api/auth/me",
    "/api/photos/../../evil",
  ])
    assert.throws(() => mediaUrl("https://mirror.example", path));
});
test("transport sends bearer auth, handles 204, and exposes session expiry", async () => {
  const original = globalThis.fetch;
  const calls: [string, RequestInit | undefined][] = [];
  globalThis.fetch = (async (url, options) => {
    calls.push([String(url), options]);
    return new Response(null, { status: calls.length === 1 ? 204 : 401 });
  }) as typeof fetch;
  try {
    const api = new MirrorApi("https://mirror.example", "session-secret");
    await api.rotate("id/with/slash", 90);
    assert.equal(
      calls[0][0],
      "https://mirror.example/api/photos/id%2Fwith%2Fslash",
    );
    assert.equal(
      (calls[0][1]?.headers as Record<string, string>).Authorization,
      "Bearer session-secret",
    );
    assert.equal(calls[0][1]?.credentials, "omit");
    await assert.rejects(
      api.me(),
      (error) => error instanceof ApiError && error.status === 401,
    );
  } finally {
    globalThis.fetch = original;
  }
});

test("flipbook media permits only same-origin face crops", () => {
  assert.equal(
    mediaUrl("https://mirror.example", "/api/admin/faces/face-123/crop"),
    "https://mirror.example/api/admin/faces/face-123/crop",
  );
  for (const path of [
    "https://other.example/api/admin/faces/id/crop",
    "/api/admin/people",
    "/api/admin/faces/id",
    "/api/admin/faces/id/crop/extra",
    "/api/admin/faces/../crop",
  ])
    assert.throws(() => mediaUrl("https://mirror.example", path));
});
