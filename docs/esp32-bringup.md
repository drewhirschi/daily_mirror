# ESP32 camera bring-up notes

What it took to get a photo out of each board, recorded 2026-09-17 to 09-19 so
nobody has to rediscover it. The firmware itself lives in `firmware/`; the plan
is in [firmware-roadmap.md](firmware-roadmap.md).

## The two bench boards

| | ESP32-P4 board | ESP32-S3 board |
| --- | --- | --- |
| Chip | ESP32-P4 rev v1.3, 32 MB flash, 32 MB PSRAM | ESP32-S3 rev v0.2, 16 MB flash, 8 MB octal PSRAM |
| Board | Waveshare P4 family (JD9365 LCD and ES8311 codec in its BSP), ESP32-C6 radio over SDIO | Freenove ESP32-S3-WROOM CAM pinout |
| Camera | Arducam 16 MP IMX519 on MIPI CSI, I2C on GPIO 7/8, sensor at 0x1a, focus motor at 0x0c | OV5640 on DVP (XCLK 15, SDA 4, SCL 5, D0..D7 = 11, 9, 8, 10, 12, 18, 17, 16, VSYNC 6, HREF 7, PCLK 13) |
| Serial | CH343, 2 Mbaud console | CH340, 921600 baud console |
| LED | external RGB LED, GPIO 21/22/23 through 330 Ω each | onboard WS2812 on GPIO 48 |
| Button | GPIO 20 to GND | GPIO 1 to GND |

Wiring drawings: [S3](../hardware/wiring/s3_button_led.svg),
[P4](../hardware/wiring/p4_rgb_led.svg).

## Traps, in the order they bit

1. **The Arducam IMX519's focus motor is an AK7375, not a DW9714.** The
   Raspberry Pi overlay binds `asahi-kasei,ak7375`. It is a 12-bit DAC: write
   `code << 4` as a 16-bit value to register 0x00, and 0x00 / 0x40 to register
   0x02 for active / standby. With the DW9714 protocol the chip ACKs and the
   lens never moves: a focus sweep comes back perfectly flat. On our module the
   useful travel is codes 1024 to 2816, peaking near 2050 at about one metre.
2. **ISP statistics are garbage on ESP-IDF 5.4.0–5.4.3 and 5.5.0–5.5.2.** The
   prebuilt `libesp_ipa.a` expects a 5x5 white-balance sub-window grid inside
   `esp_ipa_stats_t`; those IDF versions compile it out, so every field after
   it is read from the wrong offset. Auto-exposure sees a black frame, the
   focus score is a float bit pattern. Either define
   `ISP_AWB_WINDOW_X_NUM=5` and `ISP_AWB_WINDOW_Y_NUM=5` project-wide or use
   5.5.3 or later. We use 5.5.5.
3. **ESP32-P4 silicon before rev v3.0 has no ISP black-level correction.**
   The driver returns "not supported" and esp_video swallows it, so black-level
   values in the tuning file do nothing. The sensor's pedestal then passes
   through the white-balance gains and the colour matrix and turns dark areas
   purple. A sensor solid-black test pattern comes out as exact black, which
   proves the pipeline adds nothing itself. Workaround: a gamma curve whose
   first points are zero. Real fix: a rev 3.x module. White-balance gain,
   crop and sub-window white balance are gated the same way.
4. **ESP-IDF 5.5.5 defaults the P4 minimum chip revision to v3.1**, and
   esptool then refuses to flash a rev 1.3 board. A failed flash leaves the old
   firmware running, which looks like "my change did nothing". Set
   `CONFIG_ESP32P4_SELECTS_REV_LESS_V3=y` and `CONFIG_ESP32P4_REV_MIN_100=y`,
   and always check for "Hash of data verified".
5. **The sensor has no reset line on the Pi-style connector**, so register
   state such as a test pattern survives reboots. The driver clears it on every
   mode set.
6. **Tuning borrowed from another sensor looks terrible.** The first P4 frames
   used an IMX708 file: strong colour matrix, contrast 130, heavy sharpening,
   a white-balance gain step of 0.5 that froze the gains, and gain allowed to
   reach 16x. The current file is a neutral starting point, not a calibration.
7. **Average metering blows out a bright subject in a dark room.** Use
   highlight-priority metering on the P4; on the OV5640 use manual exposure or
   more light.
8. **OV5640 autofocus needs its firmware blob loaded at boot** (about 4 KB to
   0x8000 over I2C), then command 0x03 and wait for status 0x10. A delay of
   `pdMS_TO_TICKS(5)` is zero ticks at the default 100 Hz tick rate, which
   turns a polling loop into a 0.6 s busy-wait that gives up too early.
9. **The default access-point subnet is 192.168.4.0/24.** That collides with
   this house's LAN, so the device's fallback access point uses 10.10.0.1.
10. **The P4's C6 radio ships with old firmware** (reports 0.0.0). Scanning and
    joining work; Espressif warns of RPC timeouts and it can be updated from
    the P4 over SDIO.
11. **If configure fails with "Missing required kconfig option after retry"**,
    run `idf.py fullclean` and delete `sdkconfig` and `dependencies.lock`.
12. **The P4's camera ribbon is marginal.** "Failed to detect camera sensor
    with address=1a", or frames that suddenly go solid black, mean reseat the
    cable before suspecting software.

## Host setup on the dev machine

- ESP-IDF v5.5.5 in `~/esp/esp-idf`, toolchains for `esp32p4` and `esp32s3`.
- Serial ports need the `uucp` group on Arch. Without logging out,
  `echo '<command>' | newgrp uucp` runs one command with the group.
- Espressif's clang wants a legacy `libxml2.so.2`; only the Rust spike needed
  it.
