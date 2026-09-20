# Daily Mirror camera firmware

One ESP-IDF project, two boards, a board-adapter layer between them.

| Board | `MIRROR_BOARD` | Camera | Focus | Status LED | Wi-Fi |
| --- | --- | --- | --- | --- | --- |
| Waveshare ESP32-P4-WIFI6 | `p4_imx519` | Arducam IMX519 over MIPI-CSI (esp_video + ISP), 1920×1080, hardware JPEG | AK7375 VCM, continuous contrast-detect AF in the ISP | discrete RGB LED on three LEDC PWM channels | ESP32-C6 over esp_hosted/SDIO |
| Freenove ESP32-S3-WROOM CAM | `s3_ov5640` | OV5640 over DVP (esp32-camera), 1600×1200, JPEG from the sensor | OV5640 MCU firmware blob, single-shot AF | one WS2812 | on-chip |

Everything above the adapter is shared: `app_main`, the press flow, the admin
HTTP server, the uploader, Wi-Fi bring-up with a SoftAP fallback, settings and
mDNS.

---

## Build

```sh
source ~/esp/esp-idf/export.sh            # ESP-IDF v5.5.5

idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 build
idf.py -B build_s3 -DMIRROR_BOARD=s3_ov5640 build
```

or, from the repository root, `just fw-build-p4` / `just fw-build-s3`.

### How the board is chosen

`-DMIRROR_BOARD=<name>`, and nothing else. It is a cached CMake variable, so it
only has to be passed the first time a build directory is configured; after
that `idf.py -B build_p4 build` is enough.

One variable sets three things that must not drift apart, in
`firmware/CMakeLists.txt` before `project()`:

1. `IDF_TARGET` — `esp32p4` or `esp32s3`.
2. `SDKCONFIG_DEFAULTS` — `sdkconfig.defaults` then
   `boards/<board>/sdkconfig.defaults`.
3. The Kconfig choice `MIRROR_BOARD_*`, set in that per-board defaults file.
   `components/mirror_board/CMakeLists.txt` compiles exactly one
   `src/board_*.c` from it.

`sdkconfig` is written **into the build directory**, so the two boards'
configurations coexist and building one never invalidates the other.
`dependencies.lock` cannot be redirected the same way — the component manager
insists on writing it beside the manifest — so it lives in `firmware/`, is
rewritten whenever you switch boards, and is git-ignored.

Both are ignored along with `build*/`, `managed_components/` and anything named
`wifi_credentials.h` or `*.sdkconfig`; see `.gitignore`.

### Credentials, and why none are in this directory

A device with no settings comes up on its own setup network and is configured
through `/config`. Nothing in the repository ever needs an SSID, a password or
a token.

For a bench unit that should land straight on the house network, put the
build-time defaults in a file **outside the repository** and point at it:

```sh
# ~/esp/bench.sdkconfig — never inside the repo
CONFIG_MIRROR_DEFAULT_WIFI_SSID="..."
CONFIG_MIRROR_DEFAULT_WIFI_PASS="..."
CONFIG_MIRROR_DEFAULT_SERVER_URL="https://..."
CONFIG_MIRROR_DEFAULT_UPLOAD_TOKEN="..."

idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 \
       -DMIRROR_EXTRA_SDKCONFIG=~/esp/bench.sdkconfig build
```

It is applied last, after the board defaults. The build refuses a path inside
the repository. Note these are **build-time fallbacks only**: anything stored
through `/config` wins, and they are baked into the image in plain text, so
they are for the bench, not for a product.

## Flash and monitor

```sh
cd build_p4 && python -m esptool --chip esp32p4 -p /dev/ttyACM0 -b 921600 \
    --before default_reset --after hard_reset write_flash "@flash_args"
cd build_s3 && python -m esptool --chip esp32s3 -p /dev/ttyUSB0 -b 921600 \
    --before default_reset --after hard_reset write_flash "@flash_args"
```

**Always check the output for `Hash of data verified`.** A failed write is not
loud: the board simply keeps running the previous firmware, and you spend the
next hour debugging code that is not on it.

Console baud is **2,000,000** on the P4 (CH343 bridge) and **921,600** on the
S3 (CH340). `just fw-monitor-p4` / `just fw-monitor-s3`.

On a machine whose user is not in the serial group for this session, prefix
serial commands with `newgrp`:

```sh
echo '<command>' | newgrp uucp
```

## What it does

Start-up order is `log ring → settings → LED → camera → network → server →
mDNS → upload spool → button`.
The camera comes up before the network so the ISP's AE/AWB/AF convergence,
which needs a few seconds of frames, overlaps the Wi-Fi association instead of
following it. A camera that fails to initialise does **not** stop the rest:
`/config` is how someone fixes whatever is wrong, so it has to be served.

### Endpoints (port 80)

| | |
| --- | --- |
| `GET /` | status page, titled with the board name |
| `GET /snapshot.jpg` | focus, capture, send — the live scene |
| `GET /last.jpg` | the last committed press photo |
| `POST /press` | run the press flow |
| `POST /upload` | capture straight into the upload spool |
| `GET /stats` | plain text: board, camera, network, heap, and the spool |
| `GET /logs` | plain text: the last ~8 KB of the console log |
| `POST /debug/upload-block?on=1` | make every upload fail, for testing retry |
| `GET /config`, `POST /config` | the settings form, from `mirror_config` |

### The press flow

The Pi's, on this hardware (`device/src/main.rs`; pattern names from
`crates/mirror-core/src/ring.rs`):

```
countdown  AmberSlowPulse       4 × 350 ms on / 650 ms off — focus, AE/AWB
                                settle and the frame flush all run UNDER these
shutter    WhiteFlash           160 ms, at the instant the frame is taken
error      RedTriplePulse       then back to whatever is underneath

uploading  GreenSlowPulse       the spool has work in hand
sent       GreenTripleFlash     once, when the queue empties
ready      SolidWhite
```

Button: active low to GND with the internal pull-up, 60 ms debounce, 20 ms
poll. The press ends when the photo is on flash; the upload is not part of it.

**Deliberate divergences from `crates/mirror-core/src/ring.rs`**, both about
making one glance answer one question:

- Uploading is **green**, not `BlueSlowPulse`. Blue keeps its other meaning —
  joining a network, claiming — so the ring distinguishes "talking to the
  network to get set up" from "your photo is on its way".
- There is no `GreenFlash` at commit. The white shutter flash is the cue that
  the photo was taken, and a second flash a moment later read as noise.

The old flow put `AmberRapidPulse` *after* the countdown while autofocus and
the frame flush ran — about three seconds of fast blinking between "3, 2, 1"
and the actual photograph, so the countdown counted down to nothing. That work
now happens during the blinks, with `board_camera_prepare()` given the
countdown as a budget: if focus has not locked by the last blink the shutter
fires anyway and `af_state` records `searching`.

**Layers.** `mirror_ring` renders the topmost active layer of BASE (idle,
pairing, claiming), UPLOAD (the spool) and CAPTURE (a press), so a press
pre-empts the upload pulse instantly and the pulse resumes by itself
afterwards, with no caller having to know what to restore. Pairing patterns on
BASE outrank UPLOAD — "press the button to confirm" must not be painted over
by a photo going out — and do not need to outrank CAPTURE, because
`mirror_pair_confirm()` swallows the press before a capture can start. A
second press during a countdown is ignored (`mirror_press_busy()`); a press
while an upload is in flight starts its countdown immediately, since the
spool's lock is held only while a file is written, never across an upload.

### Networking

Station from the stored SSID, retrying **forever** — 1 s for the first five
attempts, then 10 s. If there is no SSID, or no address within
`MIRROR_STA_TIMEOUT_S` (60 s), the device also brings up a setup access point:

- SSID `mirror-<last 3 MAC bytes>`, the same name mDNS publishes.
- **10.10.0.1/24**, not Espressif's default 192.168.4.0/24 — this household's
  LAN is 192.168.4.0/22 and the two would overlap, leaving a phone that knows
  both networks able to reach neither reliably.
- WPA2 with `MIRROR_AP_PASSWORD`, default **`dailymirror`** (set it under
  *Daily Mirror application* in menuconfig). Under 8 characters and the network
  comes up open. This is a bench default in plain sight, not a secret; a
  shipping product should derive a per-device password.
- The same admin server, so `/config` is reachable at
  `http://10.10.0.1/config`.
- With an SSID stored the mode is APSTA, so the station keeps trying
  underneath and the device rejoins on its own once the credentials or the
  router are fixed.

### Uploads

`POST {server_url}/api/uploads` with `Authorization: Bearer <upload_token>` →
grant `{url, method, headers, complete_url}` → PUT/POST the JPEG with the
grant's headers → `POST complete_url`. The same contract as the Pi. The bearer
token is attached to the second step **only when the target is the same origin
as `server_url`**, so a signed object-storage URL never sees it.

TLS verifies against the certificate bundle (`esp_crt_bundle_attach`). The
bench firmware this came from used `CONFIG_ESP_TLS_SKIP_SERVER_CERT_VERIFY`;
that is deliberately gone.

The JPEG is streamed to the grant's URL from the spool file in 8 KB chunks out
of PSRAM. That is not tuning: the internal-heap low-water mark across a TLS
upload is around 50-70 KB, and a 386 KB JPEG held in internal RAM alongside
mbedTLS' buffers does not fit.

The grant body carries an optional `capture` object with the provenance of
that frame — see **The upload spool** below.

### The upload spool

`mirror_spool.c`, on a 3.5 MB LittleFS partition in the flash tail. Every
capture is written there first; a single drain task sends them oldest-first,
one at a time.

- **Retry.** 30 s doubling to 15 min, ±20% jitter, reset on
  `IP_EVENT_STA_GOT_IP` — the failure is nearly always the network having been
  away, and once it is back there is no reason to sit out the rest of a
  fifteen-minute backoff.
- **Power loss.** Entries are written to `tmp.dm` and renamed into `q/`, so a
  file under `q/` was written all the way through. Start-up deletes a stale
  `tmp.dm` and discards anything too short to hold a header.
- **Full.** The *oldest* entry is dropped and counted in `drops`. Roughly a
  dozen 1600×1200 JPEGs fit.
- **No clock, no upload.** A capture taken before SNTP answered would be filed
  under 1970, which the server rejects, so it is held and stamped with the
  time the clock first becomes valid.
- **4xx is not "retry forever".** `401`/`403` stops the drain entirely and
  shows `token_bad=1` in `/stats` — every photo is kept, because retrying
  cannot help and deleting would not be ours to do. `400`/`409`/`413` moves
  that one photo to `r/` (at most 2 kept) so one bad file cannot wedge the
  queue behind it. Everything else retries.

`/stats` reports `spool_count`, `spool_bytes`, `spool_capacity`,
`oldest_age_s`, `uploads_ok`, `uploads_failed`, `drops`, `rejected`,
`next_retry_s`, `last_http_status`, `last_error`, `token_bad`, and the last
capture's metadata.

To test the retry path without touching the stored claim:

```sh
curl -X POST 'http://mirror-4a30e4.local/debug/upload-block?on=1'   # photos pile up
curl -X POST 'http://mirror-4a30e4.local/debug/upload-block?on=0'   # they drain, in order
```

It is RAM-only, so a reboot clears it, and it can only make uploads fail.

### Capture metadata

The `capture` object on the grant request, all fields optional — anything the
board cannot read is **omitted**, never guessed:

```json
{"firmware_version":"0d4bb7b","sensor":"ov5640","width":1600,"height":1200,
 "jpeg_quality":89,"exposure_us":100682,"analog_gain":2.250,"mean_luma":34,
 "af_state":"focused","trigger":"button","capture_source":"device",
 "captured_at":"2026-09-20T03:16:03Z"}
```

The sensor registers are read in the board adapter immediately after the frame
is grabbed, behind `board_camera_capture_info_t`, so they describe *that*
frame — auto-exposure moves between frames. The S3 reads exposure
(`0x3500-0x3502`), gain (`0x350A/0x350B`), mean luma (`0x56A1`) and the AF
firmware status (`0x3029`); the P4 adapter returns "unknown" for all of them
for now, because on that board they live inside the ISP's closed-loop 3A
rather than in sensor status registers.

**The exposure line time is measured, not derived.** The OV5640's exposure is
in sixteenths of a row, and the row time is `HTS / (the sensor array clock)` —
which is *not* the DVP PCLK, and the PLL formula in esp32-camera's
`calc_sysclk()` gives an answer that is out by exactly 2×. So `board_s3_ov5640.c`
times six frames at start-up and divides by VTS. On this bench, at
1600×1200 JPEG:

| | |
| --- | --- |
| HTS / VTS | 2844 / 1968 |
| register-derived (`HTS/PCLK`, PCLK 12.5 MHz) | 227.52 µs |
| **measured** (223.9 ms frame / 1968 rows) | **113.77 µs** |

The measured figure implies a 25 MHz array clock, i.e. 2× the DVP PCLK, which
is what the datasheet's 8-bit DVP output mode does. The measurement is used;
the derived value is logged next to it so the two can be compared in the
field. Both appear in `/logs` on every boot.

### /logs

`esp_log_set_vprintf` tees the last 8 KB of log text into a PSRAM ring, served
as `text/plain` at `GET /logs`; serial output is unchanged. ISR context is
skipped (PSRAM is unreachable with the cache off) and the ring is guarded by a
spinlock rather than a mutex, because the log macros are reachable from an
ISR. The boot banner carries the firmware version, the reset reason and the
clock state.

No credential is ever logged: presigned URLs are printed with their query
string replaced by `?<redacted>`, and neither the device token, the Wi-Fi
password nor a claim token appears in any log line.

## Configuration

`idf.py -B build_p4 menuconfig`, under:

- **Daily Mirror board** — the board choice, button GPIO, JPEG quality, LED
  pins, camera pins, and the capture flash. Every pin below is a Kconfig
  option with these defaults.

The **capture flash** is `MIRROR_FLASH_GPIO`, **GPIO 47** on the S3 (`-1`, and
so disabled, on the P4). It comes on for the last `MIRROR_FLASH_LEAD_MS`
(1500 ms) of the press countdown and goes off the instant the frame is in
hand. That lead is not padding: the frame flush that settles auto-exposure
runs *after* the light is on, so AE meters the lit room rather than the dark
one, and the light is held rather than strobed because a pulse shorter than a
frame lights only part of a rolling-shutter readout — both written up in
`docs/burst-processing-experiment.md`. The board layer arms a 6 s one-shot on
every turn-on and the pin is driven off at start-up, so no error path, abort or
reset can leave the light on. `board_flash_set_level()` already takes a 0–255
level, so LEDC dimming drops in when the MOSFET stage arrives.
- **Daily Mirror application** — firmware version, station timeout, AP
  password, `MIRROR_DEBUG_ISP_STATS`.
- **Daily Mirror settings defaults** — the `MIRROR_DEFAULT_*` fallbacks.
- **Camera Sensor (IMX add-on)** — IMX519 mode index, `CAMERA_IMX519_DEBUG_LOG`.

### Wiring

**ESP32-P4 + IMX519**

| Signal | GPIO |
| --- | --- |
| Camera I2C SDA / SCL | 7 / 8 (IMX519 at 0x1a, AK7375 VCM at 0x0c) |
| RGB LED R / G / B | 21 / 22 / 23, one 330 Ω per colour, LEDC PWM |
| LED common leg | 3V3 — `MIRROR_LED_COMMON_ANODE=y` (outputs inverted) |
| Button | 20, to GND |
| Console UART | 37 / 38 at 2,000,000 baud |

Taken on this board and not available: 7/8 camera I2C, 9–13 + 53 I2S, 14–19 +
54 the C6 link, 24/25 USB, 37/38 UART, 39–44 SD. Check any change against the
header silkscreen.

**ESP32-S3 + OV5640** (Freenove ESP32-S3-WROOM CAM)

| Signal | GPIO |
| --- | --- |
| XCLK | 15 |
| SCCB SDA / SCL | 4 / 5 |
| D0…D7 | 11, 9, 8, 10, 12, 18, 17, 16 |
| VSYNC / HREF / PCLK | 6 / 7 / 13 |
| WS2812 | 48 |
| Button | 1, to GND |

The module is mounted upside down on the bench rig, hence `MIRROR_CAM_VFLIP=y`.

### Debug aids (off by default, not deleted)

- `MIRROR_DEBUG_ISP_STATS` — esp_ipa and esp_video at DEBUG: every exposure
  decision, white-balance gain and focus score. This is how the IMX519's focus
  band and the purple-blacks pedestal were found. P4 only in practice.
- `CAMERA_IMX519_DEBUG_LOG` — the sensor-side exposure and gain writes.

## Partitions

`partitions.csv`, one table for both boards: `nvs`, `phy_init`, `otadata`, two
6 MB OTA app slots, and a 3.5 MB `spool` LittleFS partition. There is no OTA
client yet; the layout is laid down now because changing a partition table
later means erasing NVS and losing every deployed device's settings. 15.6 MB
total, which fits the smaller board (16 MB on the S3, 32 MB on the P4).

```
nvs        data  nvs       0x009000    24K
phy_init   data  phy       0x00f000     4K
otadata    data  ota       0x010000     8K
ota_0      app   ota_0     0x020000     6M
ota_1      app   ota_1     0x620000     6M
spool      data  littlefs  0xc20000  3584K
```

`spool` was **appended** into flash that was already unallocated, so every
offset above it is byte for byte what it was. That matters: `nvs` holds the
`device_token` a claim issued, and moving it by one byte un-claims every
device in the field. Flashing the new table writes only the sector at
`0x8000`, which ends exactly where `nvs` begins.

## Adding a new board

Everything hardware-specific is behind `mirror_board.h`. The app in `main/`
never includes `esp_camera`, `esp_video`, `led_strip` or `ledc`, so a new board
is one adapter file, one defaults file, and a checklist.

### 1. The adapter: `components/mirror_board/src/board_<name>.c`

Four pieces, and nothing else:

| piece | functions |
| --- | --- |
| identity | `board_name`, `board_id` |
| camera | `board_camera_init`, `board_camera_prepare_focus`, `board_camera_prepare_settle`, `board_camera_capture`, `board_camera_release`, `board_camera_last_size`, `board_camera_capture_info`, `board_camera_status` |
| button | `board_button_gpio` |
| signal LED | `board_led_init`, `board_led_set_rgb` |
| capture flash | `board_flash_init`, `board_flash_available`, `board_flash_set`, `board_flash_on` |
| radio | `board_net_init` |

Every one must exist even when the hardware does not — a board with no flash
returns `false` from `board_flash_available()` and makes the other three
no-ops, which is exactly what `board_p4_imx519.c` does. Return "unknown"
values from `board_camera_capture_info()` rather than zeros: a missing field is
omitted from the upload, a fabricated one is a lie nothing downstream can see
through.

`board_camera_prepare_focus()` and `board_camera_prepare_settle()` are split
because the flash comes on between them (see **The press flow**). A board whose
pipeline runs continuously, like the P4's, can leave both empty.

### 2. `boards/<name>/sdkconfig.defaults`

The one file that carries values. `components/mirror_board/Kconfig` only
*declares* options, all defaulting to `-1` or `n`, so anything this file
forgets is absent rather than wrong. It sets:

- `CONFIG_IDF_TARGET` and `CONFIG_MIRROR_BOARD_<NAME>=y`.
- Every pin and polarity for the four pieces above.
- Flash size, and **PSRAM type and speed** — quad or octal, and the clock. The
  camera's frame buffers and the log ring live there; get this wrong and the
  symptom is an allocation failure a long way from the cause.
- Console UART baud, if the board's USB bridge will not take the default.
- **BLE**, if the chip has it: NimBLE plus
  `ESP_PROTOCOMM_SUPPORT_SECURITY_VERSION_2`. Without a Bluetooth radio
  `mirror_pair_start()` reports that the board cannot pair and the device has
  to be set up through `/config` on the fallback access point — a working
  path, but a different one, so decide it deliberately.

### 3. Wiring the name up

- `CMakeLists.txt`: add the name to `MIRROR_BOARDS` and map it to an
  `IDF_TARGET`.
- `components/mirror_board/CMakeLists.txt`: compile the new adapter under its
  `CONFIG_MIRROR_BOARD_*`.
- `justfile`: a `fw-build-<name>` / `fw-flash-<name>` pair, and a build
  directory of its own (`build_<short>`), since `sdkconfig` is per board and
  lives in the build directory.
- `partitions.csv` is shared. Check the **total fits the new board's flash**
  (15.6 MB today) and size the `spool` partition for it — it is the only
  partition that is a judgement call rather than a requirement.

### 4. Bring-up checklist

In this order; each step depends on the one before.

1. **Boots.** Serial shows the banner, the board name, and `boot: ... reset=`.
2. **`/stats`** answers: board id, camera line, IP, heap. `camera=` must not
   say "not started".
3. **Capture.** `POST /press` returns `captured`; `/last.jpg` is a whole image.
4. **Countdown and LED.** Four amber blinks, one white flash at the end, and
   the shutter on the last blink — `shutter at ~4000 ms` in the log.
5. **Flash**, if fitted: the light is on for the last 1.5 s and off straight
   after. Compare `mean_luma` / `exposure_us` in `/stats` `last_capture`
   against a capture with it disabled to confirm auto-exposure adapted.
6. **Pairing**, if the board has BLE: a long press enters it, the app finds it,
   and `/stats` ends at `claimed=1`.
7. **Upload.** A capture drains: `uploads_ok` increments and
   `last_http_status=204`.
8. **Spool under failure.** `POST /debug/upload-block?on=1`, two presses,
   watch `spool_count` rise with no attempts; reset the board and confirm the
   count survives; `?on=0` and watch it drain oldest-first.

### Standalone operation

The device is meant to run from a wall supply with nothing attached, so:

- **Nothing waits on the console.** Logging is plain UART with no flow control
  and the USB-serial-JTAG console is secondary, so an absent host is an absent
  reader, not a blocked writer. No code reads from stdin.
- **Brownout detection is on** (`CONFIG_ESP_BROWNOUT_DET`, level 7, plus
  `SPI_FLASH_BROWNOUT_RESET`) — a sagging USB supply resets the chip cleanly
  rather than corrupting flash mid-write.
- **Wi-Fi retries forever** (`mirror_net.c`): 1 s for the first five attempts,
  then 10 s, with no attempt limit. An access point that goes away overnight is
  rejoined on its own.
- **The spool outlives the outage.** Photos taken while the network is down sit
  on flash; `IP_EVENT_STA_GOT_IP` resets the retry backoff, so they start
  draining as soon as the address comes back rather than waiting out the
  remainder of a 15-minute timer.
- **A power cut costs nothing.** The queue is on LittleFS and entries are
  renamed into place, so start-up finds them and carries on.

## Traps

Every one of these cost hours on the bench.

- **The P4 on this rig is silicon rev v1.3.** ESP-IDF 5.5.5 defaults the
  minimum revision to v3.1 and esptool then refuses to flash at all
  (`requires chip revision in range [v3.1 - ...]`).
  `CONFIG_ESP32P4_SELECTS_REV_LESS_V3=y` and `CONFIG_ESP32P4_REV_MIN_100=y` are
  what make the image loadable.
- **The IPA JSON must be registered by the project CMakeLists**, between
  `include(project.cmake)` and `project()`. esp_ipa's own existence check is
  spelled `message(FETAL_ERROR ...)` — not a real CMake mode — so a wrong path
  prints a line, the build succeeds, and the camera runs with no auto-exposure
  and no white balance.
- **`idf.py fullclean` plus deleting `sdkconfig` and `dependencies.lock`** is
  the fix when configure fails with *"Missing required kconfig option after
  retry"*.
- **`vTaskDelay(pdMS_TO_TICKS(5))` is zero ticks at 100 Hz.** The OV5640 AF
  poll loops use `vTaskDelay(1)` for that reason, and the S3 build deliberately
  stays at the 100 Hz default — their iteration counts are tuned to 10 ms ticks.
- **The IMX519 module's VCM is an AK7375, not a DW9714/DW9807.** The DW97xx
  two-byte word protocol leaves the lens motionless and the focus sweep flat.
- **`Failed to detect camera sensor with address=1a` on the P4 is usually the
  ribbon**, which is marginal on this rig. Reboot once or twice before
  suspecting the firmware.
- **The IMX519 has no reset line**, so register state (e.g. a test pattern at
  0x0600) survives a reboot. The driver clears it in `set_format`.
- **`esp_read_mac(ESP_MAC_WIFI_STA)` fails on the P4 until the Wi-Fi driver has
  started** — the station MAC lives on the C6 — which is why the setup network
  is named after `esp_wifi_start()`, not before.
- **esp_hosted's SDIO mempool** does not fit alongside the camera stack at the
  default queue sizes, and the failure is the *idle task's* stack allocation
  returning NULL inside `vTaskStartScheduler`, before `app_main`, with nothing
  in the log mentioning Wi-Fi. Halved queue sizes fix it;
  `ESP_HOSTED_MEMPOOL_PREFER_SPIRAM` also boots but stalls every response
  larger than the initial congestion window.
- **esp_ipa ABI on older ESP-IDF**: below v5.4.4 / v5.5.3 the AWB sub-window
  macros are undefined, `esp_ipa_stats_t` is 400 bytes short of what the
  prebuilt library expects, and autofocus silently scores unrelated heap.
  `firmware/CMakeLists.txt` defines them for those versions.

## Layout

```
firmware/
  CMakeLists.txt              board selection, IPA JSON registration, ABI guard
  sdkconfig.defaults          common
  boards/<board>/sdkconfig.defaults
  partitions.csv              two OTA slots
  main/                       app_main, press flow, HTTP admin, uploader, net
  components/
    mirror_board/             the adapter interface + one src per board
    esp_cam_sensor_imx/       vendored IMX519 + AK7375 (see its README)
    mirror_config/            NVS settings and the /config form
    mirror_mdns/              mirror-xxxxxx.local and _dailymirror._tcp
```

## Licences

The project's own license is declared at the repository root. What in this
directory came from elsewhere, and under what terms, is recorded in
[NOTICE](NOTICE). The two items that need a decision before hardware is sold
are the GPL-2.0 IMX519 register tables and the OV5640 autofocus microcode;
both are described there and in `docs/firmware-roadmap.md`.
