const VERSION = "daily-mirror-v2";
const DERIVED_IMAGE_CACHE = `${VERSION}-derived-images`;
const MAX_DERIVED_IMAGES = 240;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil((async () => {
  const cacheNames = await caches.keys();
  await Promise.all(cacheNames
    .filter((name) => name.startsWith("daily-mirror-") && name !== DERIVED_IMAGE_CACHE)
    .map((name) => caches.delete(name)));
  await self.clients.claim();
})()));

// Originals and application pages remain network-only. The small, derived
// thumbnails and face crops are safe to reuse on this private device and make
// the mobile timeline usable without downloading every frame on every visit.
self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || !isDerivedImage(request.url)) return;
  event.respondWith(cacheFirstDerivedImage(request));
});

self.addEventListener("message", (event) => {
  if (event.data?.type === "CLEAR_PRIVATE_MEDIA") {
    event.waitUntil(caches.delete(DERIVED_IMAGE_CACHE));
  }
});

function isDerivedImage(value) {
  const url = new URL(value);
  if (url.origin !== self.location.origin) return false;
  return /^\/api\/photos\/[^/]+\/thumbnail$/.test(url.pathname)
    || /^\/api\/admin\/faces\/[^/]+\/crop$/.test(url.pathname);
}

async function cacheFirstDerivedImage(request) {
  const cache = await caches.open(DERIVED_IMAGE_CACHE);
  const cached = await cache.match(request);
  if (cached) return cached;

  const response = await fetch(request);
  if (response.ok && response.headers.get("content-type")?.startsWith("image/")) {
    await cache.put(request, response.clone());
    await trimCache(cache);
  }
  return response;
}

async function trimCache(cache) {
  const keys = await cache.keys();
  await Promise.all(keys.slice(0, Math.max(0, keys.length - MAX_DERIVED_IMAGES)).map((key) => cache.delete(key)));
}
