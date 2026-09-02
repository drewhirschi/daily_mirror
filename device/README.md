# Daily Mirror device

Rust owns the complete Raspberry Pi interaction loop: camera invocation,
durable local queue, upload retry, button debounce, and the three status LEDs.
The camera itself is invoked through Raspberry Pi's supported
`rpicam-still` command (falling back to `libcamera-still` on older images).

## Bring up the camera before wiring GPIO

```sh
cp .env.example .env
cargo run --release -- capture-once --no-upload
cargo run --release -- upload /path/to/test.jpg
cargo run --release -- retry
```

`capture-once` writes to the durable queue first. With `--no-upload`, it prints
the resulting path and leaves the file there. Without it, the device uploads
the image and removes the local queue copy only after a successful server
response.

Every upload first requests `POST /api/uploads` with the capture ID, JPEG size,
and bearer token. A local server sends the device back to its filesystem upload
route; production returns a short-lived R2 `PUT` URL. The Pi never stores R2
credentials and requests a fresh URL for every retry.

Use `DAILY_MIRROR_CAMERA_ARGS` for sensor-specific tuning after inspecting the
ArduCam. The default is:

```text
--nopreview --timeout 3000 --encoding jpg --quality 95 --autofocus-mode continuous --autofocus-range normal --autofocus-speed fast --autofocus-window 0.2,0.15,0.6,0.7 --metadata-format json --metadata -
```

## GPIO service

Pins use BCM numbering. Defaults are button 2, green 17, yellow 27, and red 22;
all can be changed in `.env`. The button is active-low with its internal pull-up
enabled. LEDs are active-high and must use appropriate current-limiting
resistors.

```sh
cargo run --release -- run
```

- Green: solid when ready, slowly breathing while the JPEG is finalized and
  uploaded, and two quick flashes when the complete workflow succeeds.
- Yellow: three one-second countdown pulses only.
- Red: a capture/upload error or at least one photograph awaiting retry.

The camera process and continuous autofocus start before the first yellow pulse.
Focus keeps tracking throughout the countdown, allowing the person who pressed
the button to move into the expected 3–5 foot portrait position. The default
focus window covers the central 60% of the frame width and 70% of its height,
rather than only the camera stack's middle third. Yellow stays solid after the
third pulse only until the actual full-resolution frame arrives, so it remains
an unambiguous “hold still” indicator. Green starts only after the shutter,
breathes during local JPEG validation and upload, flashes twice on success,
then returns to solid ready. Yellow is never reused for background work.

Each normal capture also writes the camera metadata JSON to standard output,
which makes fields supplied by the camera stack such as `AfState`,
`LensPosition`, and `FocusFoM` available in the service journal. Available
fields depend on the installed camera and camera software.

The physical button always takes a picture. Holding it cannot trigger multiple
captures because the service waits for release and debounces both edges.

`run` also starts a small, server-rendered Axum admin page at
`http://<pi-address>:8080`. It shows camera, queue, GPIO, and process status and
mirrors the green, yellow, and red GPIO indicators in real time. It also reports
CPU temperature, one-minute load, memory use, and used/available storage for the
durable queue's filesystem. Status changes are sent over `/api/events` using
server-sent events; the page automatically reconnects and keeps a slow status
request as a fallback. It also has controls to take a picture, retry pending
uploads, and hold any one physical LED on for wiring diagnostics. The LED test
remains active until Restore status is chosen or the physical button is pressed.
Configure the listener with
`DAILY_MIRROR_ADMIN_BIND`. The prototype has no login and should remain on a
trusted LAN; it is intentionally independent of the NextRS archive/gallery.

`GET /healthz` is the lightweight readiness probe. It returns HTTP 200 when the
service is initialized, the camera command exists, uploads are configured, and
the device is not in an error phase; otherwise it returns HTTP 503 with the same
small JSON diagnostic body. The response includes `software_version` from the
running Rust package. It does not capture a photograph.

## Camera lab

The admin's camera lab forces the IMX519's full-field 2328×1748 binned sensor
mode, then scales it to a persistent 960×720 MJPEG viewfinder in memory. It
can adjust exposure compensation, brightness, contrast, saturation, sharpness,
denoising, white balance, metering, exposure profile, autofocus range/speed,
and optional manual shutter/gain. Applying settings restarts an active preview
so the camera algorithms settle under the new configuration.

The lab also provides persistent 0°/180° rotation and horizontal or vertical
mirroring. Applying settings writes those mounting controls to
`data/camera-settings.json`; the service reloads them at startup and applies
them to the live preview, full-resolution test snaps, and normal button
captures. Override the file location with
`DAILY_MIRROR_CAMERA_SETTINGS_PATH` when packaging the device differently.

`Test snap · no upload` stops preview, captures one 4656×3496 JPEG with the lab
settings, and retains only its bytes in service memory. It does not touch the
durable queue, local disk, or gallery upload endpoint. Starting preview again or
pressing the physical button safely releases/reclaims the camera; a physical
capture always stops lab preview before running the normal workflow.
