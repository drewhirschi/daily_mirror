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
denoising, white balance, metering, exposure profile, manual shutter/gain, and
the complete focus strategy. Focus controls include continuous, single, or
manual modes; range and speed; normalized autofocus-window coordinates; direct
lens position; and one-click fixed-focus presets for 3, 4, and 5 feet. The live
image overlays the selected autofocus region. Starting preview or taking a test
snap automatically applies every visible control, while Apply settings restarts
an active preview so the camera algorithms settle under the new configuration.

The lab also provides persistent 0°/180° rotation and horizontal or vertical
mirroring. Applying settings writes those mounting controls to
`data/camera-settings.json`; the service reloads them at startup and applies
them to the live preview, full-resolution test snaps, and normal button
captures. Override the file location with
`DAILY_MIRROR_CAMERA_SETTINGS_PATH` when packaging the device differently.

`Apply + test snap` stops preview, captures one 4656×3496 JPEG with the visible
lab settings, and retains only its bytes and camera metadata in service memory.
The page shows autofocus state, lens position, focus score, exposure, analogue
gain, the complete metadata JSON, and a link that opens the full-resolution
image for 100% inspection. It does not touch the durable queue, local photo
storage, or gallery upload endpoint. Starting preview again or pressing the
physical button safely releases/reclaims the camera; a physical capture always
stops lab preview before running the normal workflow.

## rpi2: common-anode RGB indicator

The four-leg LED identified on rpi2 has leg 3 as common positive (3.3 V),
leg 2 green (P17 through its own resistor), leg 1 blue (P27 through its own
resistor), and leg 4 red (P22 through its own resistor). Button: SDA/P2 to GND.
Set `DAILY_MIRROR_LED_MODE=rgb-common-anode`. Keep the legacy pin environment
names: `YELLOW_LED_PIN=27` now identifies the physical blue output.

RGB mode starts all channels HIGH/off and inverts green PWM. Countdown yellow
combines red and green; ready/processing/success remain green. A queued upload
shows steady red instead of combining ready green and error red into yellow.
The admin's green/yellow/red lamps represent logical status colors; `/api/status`
also reports `led_mode`. Blue and white are reserved for later behavior.
The default `discrete` mode preserves rpi1's active-high three-LED hardware.

Select `DAILY_MIRROR_CAMERA_PROFILE=ov5647` for Camera v1 or `imx219` for
Camera v2. Omit `DAILY_MIRROR_CAMERA_ARGS` to use the profile defaults.

A matching systemd unit is in `deploy/daily-mirror-device.service`. It expects
`/home/drew/daily-mirror-device/bin/daily-mirror-device` and a mode-0600 `.env` in
`/home/drew/daily-mirror-device`. Enable the unit after installing both.
Use `DAILY_MIRROR_ADMIN_BIND=0.0.0.0:8081` for the new Pi.

## Camera profiles and local development

Two independent settings control hardware behavior and photo destination:

```text
DAILY_MIRROR_CAMERA_PROFILE=ov5647
DAILY_MIRROR_CAPTURE_MODE=local
DAILY_MIRROR_LOCAL_DIR=./data/local
```

| Profile | Camera | Full-resolution still | Focus |
| --- | --- | --- | --- |
| `imx519` | Arducam IMX519 | 4656 × 3496 | Autofocus, manual lens control |
| `ov5647` | Pi Camera v1 / Rev 1.3 | 2592 × 1944 | Fixed focus |
| `imx219` | Pi Camera v2 | 3280 × 2464 | Fixed focus |

`camera_profile.rs` holds the sensor-specific resolution, focus capabilities,
and preview-mode choices. The shared rpicam backend owns file capture, JPEG
validation, camera locking and orientation. Both normal capture and the lab
(still and preview) use the same profile; the UI disables unsupported focus
controls. Add future rpicam-compatible sensors here rather than scattering
sensor checks through the application. A USB/OpenCV camera would require a
separate capture backend, not merely another sensor profile.

Profiles are selected explicitly; they do not install kernel drivers or identify
a disconnected sensor. The default stays `imx519` for existing installations.
`DAILY_MIRROR_CAMERA_ARGS` remains an advanced override for normal capture only;
it replaces profile-generated arguments, so remove stale overrides when changing
sensors. Preview and lab stills continue to use the selected profile.

`local` mode saves button and admin captures in `data/local`, never queues or
uploads them, disables upload retries, and ignores server credentials even if
present. The admin page shows the latest 24 local photos, with full-resolution
links. This page is accessible on the LAN. Files remain until manually removed;
monitor free disk space during development. `capture-once --no-upload` also uses
this separate local directory, even when configured for upload mode.

`upload` mode retains the existing durable queue/retry behavior in `data/pending`.
Switching to upload mode does not sweep local photos into that queue. The two
directories must be distinct. The code defaults to `upload` for compatibility;
the example environment explicitly selects `local` for new development setups.
The standalone `upload` command refuses to run in local mode.

On rpi2, uploads are disabled and production credentials have been removed.
The camera is now confirmed as OV5647. Automatic detection missed it; explicitly
loading `ov5647` detected the sensor and produced a valid 2592 × 1944 local JPEG.
Boot configuration now sets `camera_auto_detect=0` and `dtoverlay=ov5647`.
Runtime detection and capture are verified; boot persistence is configured but
has not yet been tested by rebooting. The admin `camera_available` API field
remains an executable check, not a sensor probe.

### Inspect local test photos

In local capture mode, click a photo in **Local test photos** to open the
in-page viewer. Use the newer/older buttons or left/right arrow keys to move
through the listed photos; Escape or Close returns to the grid. **100%** shows
native image detail with scrolling, and **Fit** returns to the whole frame.
The Metadata button hides or shows the overlay without leaving the image.

The overlay reads embedded JPEG EXIF: exposure, ISO, reported focus distance,
camera/software, timestamp, and other supported fields, plus file size and
displayed dimensions. Missing fields are explicitly unavailable; current lab
settings are never attributed to old captures. EXIF timestamps have no assumed
timezone, and reported focus distance is not a measurement of subject distance.
This does not change capture processing or rewrite the originals.

Parser checks: `node device/tests/local_photo_viewer.test.cjs` from the repository root.
