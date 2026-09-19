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

Start-up order is `settings → LED → camera → network → server → mDNS → button`.
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
| `POST /upload` | capture and upload in one step |
| `GET /stats` | plain text: board, camera, network, heap, last upload |
| `GET /config`, `POST /config` | the settings form, from `mirror_config` |

### The press flow

The Pi's, on this hardware (`device/src/main.rs`; pattern names from
`crates/mirror-core/src/ring.rs`):

```
countdown  AmberSlowPulse    3 × 650 ms on / 350 ms off
capture    AmberRapidPulse   focus and grab
committed  GreenFlash        700 ms — the JPEG is in PSRAM, /last.jpg serves it
uploading  BlueSlowPulse     only when server_url and upload_token are set
ready      SolidWhite
error      RedTriplePulse    then back to ready
```

Button: active low to GND with the internal pull-up, 60 ms debounce, 20 ms
poll. The commit happens *before* the upload, so a failed upload never loses a
photo.

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

## Configuration

`idf.py -B build_p4 menuconfig`, under:

- **Daily Mirror board** — the board choice, button GPIO, JPEG quality, LED
  pins, camera pins. Every pin below is a Kconfig option with these defaults.
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

`partitions.csv`, one table for both boards: `nvs`, `phy_init`, `otadata`, and
two 6 MB OTA app slots. There is no OTA client yet; the layout is laid down now
because changing a partition table later means erasing NVS and losing every
deployed device's settings. 12.1 MB total, which fits the smaller board (16 MB
on the S3, 32 MB on the P4).

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
