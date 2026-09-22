import { test } from "node:test";
import assert from "node:assert/strict";
import { ApiError, MirrorApi } from "@daily-mirror/api";

const origin = "https://mirror.example";
const request = {
  user_id: "user",
  username: "drew",
  requested_at: "2026-09-22T18:00:00Z",
};

function stub(handler: (url: string, options?: RequestInit) => Response) {
  const original = globalThis.fetch;
  globalThis.fetch = (async (url, options) =>
    handler(String(url), options)) as typeof fetch;
  return () => {
    globalThis.fetch = original;
  };
}

test("requesting deletion posts with the session and returns the request", async () => {
  const seen: { url: string; method?: string; auth?: string }[] = [];
  const restore = stub((url, options) => {
    const headers = options?.headers as Record<string, string>;
    seen.push({ url, method: options?.method, auth: headers.Authorization });
    return Response.json(request, { status: 202 });
  });
  try {
    const api = new MirrorApi(origin, "keychain-token");
    assert.deepEqual(await api.requestAccountDeletion(), request);
    assert.deepEqual(seen, [
      {
        url: `${origin}/api/auth/account/deletion-request`,
        method: "POST",
        auth: "Bearer keychain-token",
      },
    ]);
  } finally {
    restore();
  }
});

test("no open request reads as null; other failures still throw", async () => {
  let restore = stub(() => new Response(null, { status: 404 }));
  try {
    assert.equal(
      await new MirrorApi(origin, "keychain-token").accountDeletionRequest(),
      null,
    );
  } finally {
    restore();
  }
  restore = stub(() => Response.json(request));
  try {
    assert.deepEqual(
      await new MirrorApi(origin, "keychain-token").accountDeletionRequest(),
      request,
    );
  } finally {
    restore();
  }
  restore = stub(() => new Response(null, { status: 500 }));
  try {
    await assert.rejects(
      new MirrorApi(origin, "keychain-token").accountDeletionRequest(),
      (error: unknown) => error instanceof ApiError && error.status === 500,
    );
  } finally {
    restore();
  }
});
