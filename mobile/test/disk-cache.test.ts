import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DiskCache,
  IMAGE_CACHE_BUDGET,
  MAX_PHOTO_BYTES,
  MAX_THUMBNAIL_BYTES,
  type CacheEntry,
  type CacheStorage,
} from "../src/cache/disk-cache";

function fixture() {
  let saved: CacheEntry[] = [];
  const files = new Map<string, number>();
  const downloads: string[] = [];
  const storage: CacheStorage = {
    load: () => saved.map((entry) => ({ ...entry })),
    save: (entries) => {
      saved = entries.map((entry) => ({ ...entry }));
    },
    exists: (entry) => files.has(entry.uri),
    remove: (entry) => {
      files.delete(entry.uri);
    },
    async download(key) {
      downloads.push(key);
      files.set(key, 3);
      return { uri: key, bytes: 3 };
    },
    clear: () => {
      saved = [];
      files.clear();
    },
  };
  return { files, downloads, storage };
}

test("repeated scrolling and a process restart reuse disk files without a download", async () => {
  const { storage, downloads } = fixture();
  const cache = new DiskCache(storage);
  await Promise.all([cache.get("photo?rev=0"), cache.get("photo?rev=0")]);
  await cache.get("photo?rev=0");
  await new DiskCache(storage).get("photo?rev=0");
  assert.deepEqual(downloads, ["photo?rev=0"]);
});

test("LRU budget removes the least recently viewed image", async () => {
  const { storage, files } = fixture();
  let tick = 100;
  const now = Date.now;
  Date.now = () => tick++;
  try {
    const cache = new DiskCache(storage, 6);
    await cache.get("a");
    await cache.get("b");
    await cache.get("a");
    await cache.get("c");
    assert.deepEqual([...files.keys()], ["a", "c"]);
    assert.equal(cache.stats.bytes, 6);
  } finally {
    Date.now = now;
  }
});

test("missing OS-evicted files redownload and revisions are independent", async () => {
  const { storage, files, downloads } = fixture();
  const cache = new DiskCache(storage);
  await cache.get("photo?rev=0");
  files.clear();
  await cache.get("photo?rev=0");
  await cache.get("photo?rev=1");
  cache.reconcile(new Set(["photo?rev=1"]));
  assert.deepEqual([...files.keys()], ["photo?rev=1"]);
  assert.equal(downloads.length, 3);
});

test("at most three downloads run, logout cancels queued and active requests", async () => {
  const { storage, files } = fixture();
  let running = 0,
    peak = 0;
  storage.download = async (key, signal) => {
    running++;
    peak = Math.max(peak, running);
    await new Promise<void>((resolve) =>
      signal.addEventListener("abort", () => resolve(), { once: true }),
    );
    running--;
    files.set(key, 3);
    return { uri: key, bytes: 3 };
  };
  const cache = new DiskCache(storage);
  const requests = Array.from({ length: 8 }, (_, i) => cache.get(String(i)));
  const settled = Promise.allSettled(requests);
  assert.equal(peak, 3);
  await cache.clear(true);
  assert.ok((await settled).every((result) => result.status === "rejected"));
  assert.equal(files.size, 0);
  await assert.rejects(cache.get("after-logout"));
});

test("oversized thumbnails cannot consume the cache or become full-resolution fallbacks", async () => {
  const { storage } = fixture();
  storage.download = async (key) => ({
    uri: key,
    bytes: MAX_THUMBNAIL_BYTES + 1,
  });
  const cache = new DiskCache(storage);
  await assert.rejects(cache.get("oversized"));
  assert.deepEqual(cache.stats, { count: 0, bytes: 0 });
});

test("a deletion during a download cannot repopulate the cache", async () => {
  const { storage, files } = fixture();
  let finish!: () => void;
  storage.download = async (key) => {
    await new Promise<void>((resolve) => {
      finish = resolve;
    });
    files.set(key, 3);
    return { uri: key, bytes: 3 };
  };
  const cache = new DiskCache(storage);
  const request = cache.get("deleted");
  cache.reconcile(new Set());
  finish();
  await assert.rejects(request);
  assert.equal(files.size, 0);
});

test("archive and flipbooks share storage without pruning each other's images", async () => {
  const { storage, downloads } = fixture();
  const cache = new DiskCache(storage);
  const thumbnail = "/api/photos/p/thumbnail?rev=1";
  const crop = "/api/admin/faces/f/crop";
  await cache.get(thumbnail);
  await cache.get(crop);
  cache.reconcile(new Set([thumbnail]), "/api/photos/");
  cache.reconcile(new Set([crop]), "/api/admin/faces/");
  assert.equal(cache.peek(thumbnail), thumbnail);
  assert.equal(cache.peek(crop), crop);
  await new DiskCache(storage).get(crop);
  assert.equal(downloads.filter((key) => key === crop).length, 1);
  cache.reconcile(new Set(), "/api/admin/faces/");
  assert.equal(cache.peek(crop), undefined);
  assert.equal(cache.peek(thumbnail), thumbnail);
  await assert.rejects(cache.get(crop));
  assert.equal(cache.peek(crop), undefined);
});

test("full photos share the one GB cache and survive refresh and restart", async () => {
  const { storage, files, downloads } = fixture();
  const original = "/api/photos/p?rev=2";
  const preview = "/api/photos/p/thumbnail?rev=2";
  const crop = "/api/admin/faces/f/crop";
  const download = storage.download;
  storage.download = async (key, signal) => {
    const file = await download(key, signal);
    const bytes = key === original ? 8 * 1024 * 1024 : file.bytes;
    files.set(file.uri, bytes);
    return { ...file, bytes };
  };
  assert.equal(IMAGE_CACHE_BUDGET, 1024 * 1024 * 1024);
  const cache = new DiskCache(storage);
  await Promise.all([
    cache.get(original),
    cache.get(original),
    cache.get(preview),
    cache.get(crop),
  ]);
  cache.reconcile(new Set([original, preview]), "/api/photos/");
  cache.reconcile(new Set([crop]), "/api/admin/faces/");
  const reopened = new DiskCache(storage);
  await reopened.get(original);
  assert.equal(downloads.filter((key) => key === original).length, 1);
  assert.equal(reopened.stats.count, 3);
  reopened.reconcile(new Set(["/api/photos/p?rev=3"]), "/api/photos/");
  assert.equal(reopened.peek(original), undefined);
  assert.equal(reopened.peek(crop), crop);
});

test("full-photo limits do not relax the thumbnail or face-crop size limits", async () => {
  const { storage } = fixture();
  storage.download = async (key) => ({
    uri: key,
    bytes: MAX_THUMBNAIL_BYTES + 1,
  });
  const cache = new DiskCache(storage);
  await assert.rejects(cache.get("/api/photos/p/thumbnail?rev=0"));
  await assert.rejects(cache.get("/api/admin/faces/f/crop"));
  await cache.get("/api/photos/p?rev=0");
  storage.download = async (key) => ({ uri: key, bytes: MAX_PHOTO_BYTES + 1 });
  await assert.rejects(cache.get("/api/photos/oversized?rev=0"));
});
