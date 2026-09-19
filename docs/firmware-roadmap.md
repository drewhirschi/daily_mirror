# Firmware roadmap

Status as of 2026-09-19. This tracks the ESP32 device firmware from the first
bench bring-up to something that can ship. It complements
[device-pairing-plan.md](device-pairing-plan.md), which owns the pairing
protocol and server contract; this file owns the device side.

## Decisions

| Decision | Choice | Date |
| --- | --- | --- |
| Language | C on ESP-IDF. Every camera, ISP, provisioning and hosted-Wi-Fi API we depend on is C, and both boards work today in C. A Rust spike (esp-idf-svc HTTP server on the S3) built and ran, so Rust stays possible later for the platform-neutral core, but it is not on the path. | 2026-09-19 |
| Boards | Two supported targets behind one app: ESP32-P4 + Arducam IMX519 (MIPI CSI, ISP on the P4, Wi-Fi via the ESP32-C6) and ESP32-S3 + OV5640 (DVP, on-sensor ISP and JPEG, native Wi-Fi and BLE). | 2026-09-19 |
| Structure | One ESP-IDF project under `firmware/esp-idf/`, beside the Rust host simulator in `firmware/host/`. Shared app in `main/`, everything board-specific behind `components/mirror_board`. | 2026-09-19 |
| First-run configuration | Settings live in NVS and are edited from the admin page. With no Wi-Fi the device raises its own access point on 10.10.0.1 and serves the same page. BLE onboarding replaces this for end users later and writes the same NVS keys. | 2026-09-19 |
| Discovery | mDNS hostname `mirror-<last 3 MAC bytes>.local` and a `_dailymirror._tcp` service with `id`, `board`, `fw`, `claimed` records. | 2026-09-19 |

## What works on the bench today

- **P4 + IMX519**: 1920x1080 through the ISP, hardware JPEG, closed-loop
  autofocus (AK7375 motor), auto exposure and white balance, Wi-Fi through the
  C6, snapshot over HTTP in under half a second.
- **S3 + OV5640**: up to 2560x1920 JPEG, the sensor's own autofocus firmware
  (focus lock in about 2.6 s), Wi-Fi, admin page, LED button, mDNS.
- **Both**: the Pi's button press flow and LED vocabulary, a kept-in-RAM last
  photo at `/last.jpg`, and the Pi's grant / PUT / complete upload contract
  (implemented, blocked on a device token for an end-to-end test).

## Milestone 1: mergeable firmware (this PR)

- [x] Unify the two bench apps into `firmware/esp-idf/` with the board adapter layer.
- [x] `mirror_config`: NVS settings and the `/config` admin form.
- [x] `mirror_mdns`: hostname and service advertisement on both boards.
- [x] Access-point fallback on 10.10.0.1 when Wi-Fi is unset or unreachable.
- [x] Two-slot OTA partition layout, so later updates never need USB.
- [x] Debug logging kept but off by default behind Kconfig switches.
- [x] Third-party notices for everything borrowed (`firmware/esp-idf/NOTICE`).
- [ ] Declare the project's own license at the repository root; the repo currently declares none.
- [ ] Fetch `/config` over the fallback access point from a phone (the dev machine has no Wi-Fi radio, so only the serial log has confirmed it).
- [ ] End-to-end upload test once a device token is in the settings.
- [x] `just` recipes for build, flash and monitor per board.
- [x] Bring-up doc and wiring schematics moved into the repo.
- [x] Correct [device-pairing-plan.md](device-pairing-plan.md): an IMX519
      driver now exists (ours), and the focus motor is an AK7375, not a DW9714.

## Milestone 2: image quality

- [ ] Tune the P4 ISP against the real light panel: colour matrix, white
      balance ranges, gamma, sharpening, denoise-by-gain. Today's file is a
      neutralised copy of another sensor's tuning.
- [ ] Feed the JPEG encoder YUV422 instead of 16-bit RGB to remove banding.
- [ ] Test the P4's full-field 2328x1748 mode against the ISP's line-width
      limit; 1080p is currently an 82% crop of the sensor.
- [ ] Like-for-like comparison of both boards at the mirror distance, in
      daylight and under the panel.
- [ ] Bracketed or burst capture on a press, with the server choosing or
      merging frames.
- [ ] Decide the product P4 silicon: rev 1.3 has no ISP black-level block
      (worked around with a gamma toe); rev 3.x has it.

## Milestone 3: onboarding and fleet

- [ ] BLE provisioning on the S3 (Espressif `wifi_provisioning`, security 2),
      tested first with Espressif's stock phone app.
- [ ] P4 onboarding path: BLE through the C6 once upstream supports it;
      access-point provisioning until then.
- [ ] iOS onboarding flow (separate PR): discover, provision, claim, confirm
      with a button press.
- [ ] Device claim against the server and the `claimed` mDNS flag.
- [ ] OTA client: version check, download to the idle slot, verify, reboot,
      roll back on failure. Signed images before anything leaves the house.
- [ ] Update the P4's C6 radio firmware from the P4; it ships with an old
      image that Espressif warns will cause RPC timeouts.

## Milestone 4: appliance behaviour

- [ ] Durable upload queue in flash with a wear policy (no SD card).
- [ ] Offline capture and retry rules copied from the Pi.
- [ ] Presence and health reporting to the server, including last LAN address
      as a fallback when mDNS is blocked.
- [ ] Power, thermals and enclosure checks for the chosen board.
- [ ] Decide whether `crates/mirror-core` is ported to C, bound into the
      firmware, or kept as the executable specification the C app is tested
      against.

## Open questions

- **Licensing before selling hardware.** The IMX519 register tables come from
  the Raspberry Pi kernel driver (GPL-2.0), the P4 camera component is a fork
  of an Apache-2.0 project, and the OV5640 autofocus blob is OmniVision
  firmware republished as public domain. A separate assessment covers the
  options.
- **Which board is the product.** The S3 is cheaper, simpler, has BLE today,
  and its camera arrives tuned. The P4 has the far better sensor and an ISP we
  control, at the cost of tuning work and an immature Bluetooth story.

## Bench debugging switches

These stay in the tree and default to off:

| Switch | Effect |
| --- | --- |
| `CONFIG_CAMERA_IMX519_DEBUG_LOG` | Logs every exposure and gain write the auto-exposure loop makes. |
| `CONFIG_MIRROR_DEBUG_ISP_STATS` | Raises the image-processing library's log level and dumps ISP statistics. |
| Sensor test patterns | Colour bars, solid black, solid grey; solid black proved the pipeline adds no tint of its own. |
