# Daily Mirror ESP32-P4 firmware

The product target. An ESP32-P4 with a companion ESP32-C6 radio and an
Arducam IMX519 sensor, running the same `daily-mirror-core` state machine and
driver loop that `firmware/host` runs under `cargo test`.

**This crate does not compile without the ESP-IDF toolchain**, which is why it
is excluded from the firmware workspace (`../Cargo.toml`) and from
`just check`. Everything in `src/` is an adapter skeleton with `todo!()` where
the FFI goes; the logic it feeds is already written and already tested on the
host.

## Toolchain

```sh
# 1. The Rust toolchain for Espressif targets. The P4 is RISC-V, so the
#    xtensa fork is not strictly required, but espup installs ldproxy and the
#    export script either way.
cargo install espup --locked
espup install
. $HOME/export-esp.sh          # add this to your shell profile

# 2. ESP-IDF 5.3 or newer — the first release that supports the P4 and
#    esp_wifi_remote. `embuild` will fetch and cache it on the first build,
#    or point ESP_IDF_PATH at an existing checkout.
cargo install ldproxy --locked
cargo install espflash cargo-espflash --locked

# 3. Build and flash.
cargo build --release
cargo espflash flash --release --monitor
```

The target (`riscv32imafc-esp-espidf`) comes from `.cargo/config.toml`. Note
the `f` and `c`: the P4's core has the float and compressed extensions, and
the C3/C6 target `riscv32imc-esp-espidf` is a different, non-working target
that is easy to reach for by accident.

`cargo build` also needs nightly-style `build-std`, which the espup toolchain
provides. If the build complains about `std` for an unknown target, the export
script has not been sourced in that shell.

## What the files are

| File | Job |
| --- | --- |
| `Cargo.toml` | Crate plus the `esp-idf-sys` `extra_components` entries that pull the C libraries |
| `.cargo/config.toml` | Target, linker (`ldproxy`), runner (`espflash`), `MCU`, IDF version |
| `sdkconfig.defaults` | Hosted Wi-Fi, SoftAP provisioning, ISP and JPEG, PSRAM, partition table |
| `partitions.csv` | NVS, two OTA slots, a small FAT partition |
| `build.rs` | Hands the ESP-IDF build off to `embuild` |
| `bindings.h` | The C headers bindgen turns into the FFI surface |
| `components.yml` | The same component set in `idf.py` form, for debugging the C side alone |
| `src/*.rs` | One adapter per core trait, plus `main.rs` |

## Components

Four come from Espressif's registry and one from GitHub:

- `espressif/esp_hosted` and `espressif/esp_wifi_remote` — the P4 has no
  radio. `esp_wifi_remote` presents the ordinary `esp_wifi_*` API and
  forwards it over SDIO to the C6.
- `espressif/network_provisioning` — SoftAP provisioning, the Curve25519
  handshake with proof of possession, and the custom endpoint carrying the
  claim token. BLE is the better first-run experience, but P4 BLE
  provisioning is unfinished upstream; when it lands it is a scheme swap in
  `net.rs` and one line in `sdkconfig.defaults`, with no protocol change.
- `espressif/esp_video` — MIPI CSI capture, the ISP pipeline, hardware JPEG.
- [`NB11B/esp32p4-imx519-driver`](https://github.com/NB11B/esp32p4-imx519-driver)
  — the IMX519 is not one of the sixteen sensors in `esp_cam_sensor`. This
  community port carries register tables derived from the Raspberry Pi kernel
  driver and drives the DW9714 focus motor directly. **It is GPL-derived;
  review the licence before shipping firmware built on it.**

## Bring-up order

Each step is independently verifiable, and each one that fails fails visibly
on the ring. Do not skip ahead — the camera is last for a reason.

1. **Ring and long-press.** `clock.rs`, `button.rs`, `ring.rs`. No network, no
   storage. Success: the ring breathes dim white, fills amber from 2 s into a
   hold, and goes amber-chase at 5 s. This is also the whole of the
   "the button works before Wi-Fi exists" requirement.
2. **Hosted Wi-Fi join.** `net.rs`, `HostedNet::new` and `join`, with
   credentials hard-coded. Success: an IP address in the monitor log. If this
   does not work, nothing after it can.
3. **SoftAP provisioning.** `start_provisioning`, `poll_credentials`,
   `report`. Success: the phone finds `DailyMirror-<suffix>`, completes the
   handshake, and the device reaches `AwaitingConfirm` with the ring pulsing
   amber fast. Espressif's stock SoftAP example is the fallback comparison if
   the app side is in doubt.
4. **Claim.** `claim`, plus `store.rs` NVS read and write. Success: a device
   token in NVS, the ring solid white, and the device visible in the
   household. Power-cycle: it must come back Ready without pairing again.
5. **Fixture upload.** Build with `--features fixture-camera` so `capture`
   returns a JPEG flashed into the image. This exercises the queue, the
   grant / PUT / complete sequence, and the per-device bearer token with no
   camera on the bench. Success: the photograph appears in the gallery.
6. **Camera.** `camera.rs`. Write the standalone probe first — initialize the
   IMX519, set focus, write one 1080p frame to the SD card — and only then
   wire it into the loop.

## IMX519 and 1080p

- The P4's ISP caps at **1920×1080**. That is the product resolution for v1;
  full-resolution raw bypass is not available.
- The sensor's binned mode runs at video rate, which makes burst capture cheap
  later: one press, several frames, one capture id, and the server picks the
  sharpest or stacks them. Firmware-wise that is "upload N JPEGs under one
  capture id" and needs no protocol change.
- Focus is driven directly through the DW9714 voice-coil motor over I²C, so
  the Pi rig's focus work transfers. That work's conclusion stands: autofocus
  was never the problem, low light was. Budget for the light before blaming
  the lens.
- A 1080p JPEG plus the frame buffers does not fit in internal RAM. PSRAM is
  enabled in `sdkconfig.defaults` and is not optional.

## What cannot be verified off the board

| Layer | Off-board | Confidence |
| --- | --- | --- |
| State machine, pairing, queue, upload | `firmware/host` + `cargo test` | High — this is where the bugs live, and it is covered |
| Wi-Fi join, HTTP upload, NVS | Wokwi P4, beta, virtual access point | Medium, smoke only |
| SoftAP provisioning | Wokwi's simulated hosted co-processor | Medium, smoke only |
| BLE provisioning | Nowhere; unsupported upstream on the P4 | None |
| Camera | Nowhere | None |
