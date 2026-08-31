const VERSION = "daily-mirror-v1";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

// Intentionally network-only: private gallery pages and photographs are never
// copied into a service-worker cache on the device.
self.addEventListener("fetch", () => {});
