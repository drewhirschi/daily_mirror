import { test } from "node:test";
import assert from "node:assert/strict";
import { MirrorApi, type EnrollmentStatus } from "@daily-mirror/api";
import {
  CAPTURE_ID_PATTERN,
  ENROLLMENT_POSES,
  EnrollmentUploader,
  createCaptureId,
} from "../src/enrollment";

const ORIGIN = "https://mirror.example";
const body = () => "photo-bytes";

type Call = { url: string; method: string; headers: Record<string, string> };

/**
 * Stub the three-step chain: grant, signed PUT, finalize. The grant and
 * finalize requests go through `MirrorApi`, which uses the global fetch, so the
 * stub is installed both globally and as the uploader's injected fetch.
 */
function stubChain(
  t: { after(fn: () => void): void },
  options: {
    failPut?: number;
    grantUrl?: string;
    status?: () => EnrollmentStatus;
  } = {},
) {
  const calls: Call[] = [];
  let puts = 0;
  const fetchImpl = (async (url, init) => {
    const target = String(url);
    calls.push({
      url: target,
      method: init?.method ?? "GET",
      headers: (init?.headers ?? {}) as Record<string, string>,
    });
    if (target.endsWith("/enrollment/uploads"))
      return Response.json({
        method: "PUT",
        url: options.grantUrl ?? "https://blob.example/signed/photo",
        headers: { "x-signature": "abc" },
        complete_url: "/api/uploads/id",
      });
    if (target.startsWith("https://blob.example")) {
      puts++;
      if (options.failPut && puts <= options.failPut)
        throw new TypeError("Network request failed");
      return new Response(null, { status: 200 });
    }
    if (target.endsWith("/enrollment") && (init?.method ?? "GET") === "GET")
      return Response.json(options.status?.() ?? { photos: [] });
    return new Response(null, { status: 204 });
  }) as typeof fetch;
  const original = globalThis.fetch;
  globalThis.fetch = fetchImpl;
  t.after(() => {
    globalThis.fetch = original;
  });
  return { calls, fetchImpl, putCount: () => puts };
}

const uploader = (fetchImpl: typeof fetch) =>
  new EnrollmentUploader(new MirrorApi(ORIGIN, "session-secret"), "person-1", {
    fetchImpl,
    openBody: body,
  });

test("capture ids match the Pi's YYYYMMDDTHHMMSSZ-<8 hex> convention", () => {
  assert.match(createCaptureId(), CAPTURE_ID_PATTERN);
  assert.equal(
    createCaptureId(new Date("2026-09-15T04:05:06Z"), () =>
      Uint8Array.from([0x0a, 0xbc, 0xde, 0xf0]),
    ),
    "20260915T040506Z-0abcdef0",
  );
  // Distinct captures within the same second must not collide.
  assert.notEqual(createCaptureId(), createCaptureId());
});

test("the five poses sweep lower left to lower right within the preview", () => {
  assert.deepEqual(
    ENROLLMENT_POSES.map((pose) => pose.id),
    ["lower-left", "upper-left", "center", "upper-right", "lower-right"],
  );
  for (const pose of ENROLLMENT_POSES) {
    assert.ok(pose.x > 0 && pose.x < 1, `${pose.id} x is a fraction`);
    assert.ok(pose.y > 0 && pose.y < 1, `${pose.id} y is a fraction`);
    assert.ok(pose.caption.length > 0);
  }
});

test("an upload requests a grant, PUTs the file, then finalizes", async (t) => {
  const { calls, fetchImpl } = stubChain(t);
  const uploads = uploader(fetchImpl);
  await uploads.upload(0, "file:///tmp/pose.jpg", 4096, "image/jpeg");

  assert.equal(calls.length, 3);
  const [grant, put, finalize] = calls;
  const photoId = uploads.state[0].photoId!;
  assert.match(photoId, CAPTURE_ID_PATTERN);

  assert.equal(grant.method, "POST");
  assert.equal(
    grant.url,
    `${ORIGIN}/api/household/people/person-1/enrollment/uploads`,
  );
  assert.equal(grant.headers.Authorization, "Bearer session-secret");

  assert.equal(put.method, "PUT");
  assert.equal(put.url, "https://blob.example/signed/photo");
  assert.equal(put.headers["Content-Length"], "4096");
  assert.equal(put.headers["Content-Type"], "image/jpeg");
  assert.equal(put.headers["x-signature"], "abc");
  // The session token must never reach third-party blob storage.
  assert.equal(put.headers.Authorization, undefined);

  assert.equal(finalize.method, "POST");
  assert.equal(
    finalize.url,
    `${ORIGIN}/api/household/people/person-1/enrollment/uploads/${photoId}`,
  );
  assert.equal(uploads.state[0].status, "processing");
});

test("a same-origin upload target still carries the session token", async (t) => {
  const { calls, fetchImpl } = stubChain(t, {
    grantUrl: "/api/uploads/local-target",
  });
  await uploader(fetchImpl).upload(0, "file:///tmp/pose.jpg", 64);
  const put = calls.find((call) => call.method === "PUT")!;
  assert.equal(put.url, `${ORIGIN}/api/uploads/local-target`);
  assert.equal(put.headers.Authorization, "Bearer session-secret");
});

test("a network failure retries the chain once and keeps the capture id", async (t) => {
  const { calls, fetchImpl, putCount } = stubChain(t, { failPut: 1 });
  const uploads = uploader(fetchImpl);
  await uploads.upload(2, "file:///tmp/pose.jpg", 2048);

  assert.equal(putCount(), 2, "the PUT is attempted twice");
  assert.equal(uploads.state[2].status, "processing");
  const ids = calls.filter((call) =>
    call.url.endsWith("/enrollment/uploads"),
  ).length;
  assert.equal(ids, 2, "the grant is requested again on retry");
  // Both attempts reserve the same photo, so the retry is idempotent.
  const finalize = calls.filter((call) =>
    call.url.includes("/enrollment/uploads/"),
  );
  assert.equal(finalize.length, 1);
  assert.ok(finalize[0].url.endsWith(uploads.state[2].photoId!));
});

test("a persistently failing upload gives up after one retry", async (t) => {
  const { fetchImpl, putCount } = stubChain(t, { failPut: 5 });
  const uploads = uploader(fetchImpl);
  await uploads.upload(0, "file:///tmp/pose.jpg", 2048);
  assert.equal(putCount(), 2);
  assert.equal(uploads.state[0].status, "failed");
  assert.match(uploads.state[0].message!, /try again/i);
});

test("server status merges into slots and polling stops once all are enrolled", async (t) => {
  let responses = 0;
  const photoIds: string[] = [];
  const { fetchImpl } = stubChain(t, {
    status: () => {
      responses++;
      return {
        person_id: "person-1",
        enrolled: responses > 1,
        enrolled_photos: responses > 1 ? 5 : 3,
        required_photos: 5,
        photos: photoIds.map((photo_id, index) => ({
          photo_id,
          captured_at: "2026-09-15T04:05:06Z",
          // The first response leaves two poses processing.
          status:
            responses > 1 || index < 3
              ? ("enrolled" as const)
              : ("processing" as const),
          face_count: 1,
          thumbnail_url: `/api/photos/${photo_id}/thumbnail`,
        })),
      };
    },
  });
  const uploads = uploader(fetchImpl);
  for (let index = 0; index < ENROLLMENT_POSES.length; index++) {
    await uploads.upload(index, `file:///tmp/pose-${index}.jpg`, 1024);
    photoIds.push(uploads.state[index].photoId!);
  }
  assert.equal(
    uploads.state.every((slot) => slot.status === "processing"),
    true,
  );

  const seen: number[] = [];
  const unsubscribe = uploads.subscribe((slots) =>
    seen.push(slots.filter((slot) => slot.status === "enrolled").length),
  );
  await uploads.pollStatus(0);
  unsubscribe();

  assert.equal(responses, 2, "polling stops as soon as nothing is unresolved");
  assert.equal(uploads.complete, true);
  assert.equal(uploads.enrolledCount, 5);
  assert.deepEqual(seen, [3, 5], "subscribers see each merge");
  assert.equal(
    uploads.state[0].thumbnailUrl,
    `/api/photos/${photoIds[0]}/thumbnail`,
  );
  assert.equal(uploads.nextSlotIndex(), -1);
});

test("a retake verdict explains why and frees the slot", async (t) => {
  let id = "";
  const { fetchImpl } = stubChain(t, {
    status: () => ({
      person_id: "person-1",
      enrolled: false,
      enrolled_photos: 0,
      required_photos: 5,
      photos: [
        {
          photo_id: id,
          captured_at: "2026-09-15T04:05:06Z",
          status: "retake" as const,
          face_count: 2,
          thumbnail_url: null,
        },
      ],
    }),
  });
  const uploads = uploader(fetchImpl);
  await uploads.upload(1, "file:///tmp/pose.jpg", 512);
  id = uploads.state[1].photoId!;
  await uploads.pollStatus(0);

  assert.equal(uploads.state[1].status, "retake");
  assert.match(uploads.state[1].message!, /more than one face/i);
  assert.equal(uploads.nextSlotIndex(), 0);
  uploads.retake(1);
  assert.equal(uploads.state[1].status, "empty");
  assert.equal(uploads.state[1].photoId, undefined);
});
