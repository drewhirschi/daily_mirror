# Daily Mirror productization plan

Daily Mirror's first product milestone is a dependable wall-mounted capture
appliance: power it on, press one button, receive clear physical feedback, and
retain the original photograph even when the network is unavailable. Image
enhancement remains a later, server-side concern.

## Decisions made

- Keep the Raspberry Pi implementation as the reference prototype.
- Replace the breadboard's three visible status LEDs with one full-color ring
  around a momentary button.
- Do not add photographic illumination yet. The ring is an interface, not a
  flash or fill light.
- Keep original JPEGs immutable and retain the Pi's durable retry queue.
- Deploy the NextRS gallery/API to Vercel, but upload images directly to a
  private Cloudflare R2 bucket with short-lived signed URLs.
- Keep the backend target and device credential in runtime configuration; do
  not compile either into the device binary.
- Treat an ESP32 device as a later hardware port, not as a requirement for the
  first enclosed Pi version.

## Proposed physical interaction

One addressable RGB ring can remain visually white most of the time while using
color to make exceptional states unambiguous.

| State | Ring behavior | Meaning |
| --- | --- | --- |
| Ready | Soft white, solid | The appliance is ready for a press. |
| Countdown | Three slow amber pulses | Get into position. |
| Focus and capture | Rapid amber pulses | Hold still. |
| Captured | One green flash | The JPEG is safely committed; it is safe to move. |
| Uploading | Slow blue pulse | Network transfer is in progress. |
| Complete | Return to solid white | Ready for another capture. |
| Error | Repeating red triple-pulse | The local admin page has details. |

The normal capture path must explicitly request single autofocus immediately
before capture. The current physical-button path uses an immediate still and
does not trigger `--autofocus-on-capture`; the camera-lab path already does.

### Premium button option

[ChromaTek 19 mm rugged momentary metal pushbutton with NeoPixel
ring](https://www.adafruit.com/product/5236) (Adafruit product 5236) is a close
fit. It is a panel-mount momentary switch with a full-color addressable ring,
normally-open and normally-closed contacts, a detachable seven-wire harness,
and a 19 mm cutout. It is currently listed at $19.95. Keep it as a polished
option rather than the default: the first enclosure can reuse the existing RGB
LED with an inexpensive panel-mount momentary button.

The ring uses 5 V power, ground, and one WS2812/NeoPixel data line. For a final
Pi or ESP32 assembly, use a proper 3.3 V-to-5 V logic-level shifter on the data
line rather than relying on a marginal direct connection. The switch contacts
remain electrically separate and connect to a GPIO input and ground.

## Enclosure and wiring milestone

The detailed build sequence, validation gates, and immediate parts/tool list are
in [hardware-prototyping-plan.md](hardware-prototyping-plan.md).

- Check whether the existing acrylic case actually places plastic across the
  optical path. Acrylic around the module does not affect image quality.
- Do not add a second glass *lens*. If the camera needs a protective front
  cover, use an open bezel or an anti-reflection optical glass window.
- Rigidly mount the camera module and keep the lens centered in the opening.
- Mount the 19 mm button through the front panel with its supplied gasket and
  nut.
- Replace the breadboard with a small perfboard or purpose-built carrier using
  locking connectors or screw terminals.
- Mount the Pi on standoffs, provide ventilation, strain-relieve power, and add
  wall keyholes or a removable mounting plate.
- Keep the light ring physically separated from the camera opening to avoid
  flare. A future photographic light should be a separate high-CRI, diffused
  assembly with its own driver.

## Pi service and repeatable updates

The deployed `daily-mirror-device.service` is already enabled at boot, waits
for `network-online.target`, restarts after failures, and is currently active.
That is runtime recovery, not software delivery: new binaries are still built,
copied to the Pi, installed, and restarted manually.

Adopt the sister `linkedin-challenge` repository's packaging pattern:

- Add a root `justfile` as the documented human interface.
- Add `just doctor`, `just check`, `just device-build`, `just device-deploy
  host=rpi1.local`, `just device-status`, `just server-deploy-preview`, and
  `just server-deploy`.
- Add a guarded Pi deploy script that cross-builds locally, copies a `.new`
  binary, preserves the previous binary, atomically installs the new one,
  restarts systemd, calls `/healthz`, and rolls back if health does not recover.
- Add a one-time installer that creates a dedicated directory, systemd unit,
  and root-readable environment file, then enables the service.
- Expose the binary version and build revision in `/healthz` and the admin UI so
  a deploy can prove which code is running.
- Keep automatic self-updates out of the first appliance. A one-command,
  verified operator deploy is easier to recover than an unattended updater.

Suggested runtime configuration:

```text
DAILY_MIRROR_SERVER_URL=https://daily-mirror.example.com
DAILY_MIRROR_UPLOAD_TOKEN=<per-device secret>
DAILY_MIRROR_DEVICE_ID=hallway
DAILY_MIRROR_QUEUE_DIR=/var/lib/daily-mirror/pending
```

## Vercel and R2 upload architecture

The current filesystem photo store is appropriate locally but not on Vercel.
Vercel Functions have a read-only filesystem except for temporary scratch
space, and request or response bodies are capped at 4.5 MB. A high-quality
IMX519 JPEG can cross that limit.

Use this protocol instead:

1. The Pi captures and durably queues an immutable JPEG.
2. The Pi sends the capture ID, byte length, content type, and device ID to a
   small authenticated Vercel endpoint.
3. Vercel validates the request and returns a short-lived presigned R2 `PUT`
   URL for a deterministic object key.
4. The Pi uploads the JPEG directly to private R2 storage.
5. An R2 success response allows the Pi to remove its queued copy. A retry asks
   for a new URL and writes the same object key, preserving idempotency.
6. The gallery lists capture metadata and uses authenticated or short-lived
   signed reads for originals and future derived images.

Keep the existing `PhotoStore` boundary and add an R2 implementation rather
than coupling routes directly to an S3 client. Configure R2 credentials only on
Vercel; the Pi receives operation-specific URLs and never stores the R2 secret.

The NextRS scaffold owns the Vercel adapter and deployment wiring. Bring this
repo's generated `api/index.rs`, `vercel.json`, and
`scripts/deploy-prebuilt.sh` in line with the sister repo, then wrap the
generated deploy path from the root `justfile` instead of replacing framework
files.

References:

- [Vercel Function filesystem](https://vercel.com/docs/functions/runtimes)
- [Vercel Function 4.5 MB payload limit](https://vercel.com/docs/functions/limitations)
- [Cloudflare R2 presigned URLs](https://developers.cloudflare.com/r2/api/s3/presigned-urls/)

## Later ESP32 port

The server protocol, capture IDs, state names, LED behavior, and retry rules can
remain. Most Pi device adapters cannot:

| Current Pi dependency | ESP32 replacement |
| --- | --- |
| `rppal` GPIO | `esp-idf-hal` GPIO and PWM/RMT |
| `rpicam-still` subprocess | ESP-IDF camera component through generated C bindings |
| `reqwest` + Linux TLS | `esp-idf-svc` HTTP/TLS client |
| Axum/Tokio admin server | Small ESP-IDF HTTP server or a reduced diagnostics API |
| Linux filesystem queue | Bounded flash/SD queue with explicit wear policy |
| systemd deployment | Partitioned OTA firmware with rollback |
| dotenv/CLI configuration | NVS provisioning plus build-time defaults |

Estimate this as a 60–80% rewrite of the device executable, while retaining
most server code and the conceptual state machine. Before starting, split a
small platform-neutral crate containing capture IDs, device phases, LED
patterns, and upload request/response models from the Pi-specific adapters.

An ESP32-S3 uses Espressif's DVP camera stack and officially supported sensors
top out at 5 MP in the common driver. It cannot directly reuse the Pi's MIPI
CSI-2 IMX519 module. ESP32-P4 supports MIPI CSI, an ISP, and higher-resolution
camera pipelines, but sensor/board support and Wi-Fi topology must be evaluated
against a specific development kit. Rust can use `esp-idf-svc` for Wi-Fi,
HTTP, NVS, OTA, and related services, while the camera component will likely
need generated bindings to Espressif's C APIs.

References:

- [Espressif ESP32 camera driver and supported sensors](https://github.com/espressif/esp32-camera)
- [ESP32-P4 camera interfaces and video components](https://docs.espressif.com/projects/esp-video-components/en/latest/esp32p4/Get_Started/index.html)
- [Rust ESP-IDF service wrappers](https://github.com/esp-rs/esp-idf-svc)
- [Binding ESP-IDF components such as `esp32-camera` from Rust](https://github.com/esp-rs/esp-idf-sys/blob/master/BUILD-OPTIONS.md)

## Execution order and completion checks

Current status (August 29, 2026): interaction and wiring work is intentionally
paused. The repeatable delivery commands and rollback script are implemented.
The private `daily-mirror` R2 bucket and Vercel production deployment are live
at `https://daily-mirror-pearl.vercel.app`; the gallery is password-protected;
and the Pi uses an authenticated grant to upload directly to R2. A real
4656×3496 capture was uploaded, retrieved from R2, and validated while the Pi's
durable queue returned to zero. Milestones 2–4 are complete; enclosure and
ESP32 work remain intentionally deferred.

1. **Interaction:** one RGB button works from the Pi and the real autofocus,
   capture, upload, and error states are distinguishable physically and in SSE.
2. **Repeatable Pi delivery:** a clean machine can install once and subsequent
   `just device-deploy` runs are health-checked and rollback-safe.
3. **Durable cloud storage:** local development still works; production uploads
   bypass Vercel payload limits and survive in a private R2 bucket.
4. **Vercel deployment:** preview and production deploys use the generated
   prebuilt path, health checks pass, and the Pi is provisioned with the stable
   production origin and a per-device token.
5. **Enclosed Pi appliance:** no breadboard wiring remains, the unit survives a
   reboot/network outage, and one button reliably creates one gallery image.
6. **ESP32 feasibility spike:** one selected board captures a useful JPEG,
   drives the RGB button, joins Wi-Fi, obtains a signed URL, and uploads to R2
   before committing to the full port.
