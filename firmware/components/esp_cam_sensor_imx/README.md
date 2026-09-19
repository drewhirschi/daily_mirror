# esp_cam_sensor_imx (vendored, trimmed)

Sony **IMX519** MIPI-CSI driver plus the **AK7375** autofocus VCM for the
ESP32-P4, plugging into Espressif's `esp_cam_sensor` / `esp_video` framework.

This directory is a **vendored copy**, not original work. It is kept in-tree
rather than pulled from the component registry because the IMX519 and AK7375
sources here are local additions that have not been published upstream.

## Provenance

| Path | Origin | Licence |
| --- | --- | --- |
| `CMakeLists.txt`, `Kconfig`, component scaffolding | [mushBrainDave/esp32-p4-imx-camera](https://github.com/mushBrainDave/esp32-p4-imx-camera), `components/esp_cam_sensor_imx` | Apache-2.0 (`LICENSE`) |
| `sensors/imx519/imx519.c`, `include/imx519.h`, `private_include/imx519_regs.h`, `cfg/imx519_default.json` | Written for Daily Mirror against that project's driver skeleton | Apache-2.0 |
| `sensors/imx519/private_include/imx519_settings.h` | **Register tables taken verbatim from the Raspberry Pi Linux kernel driver** `drivers/media/i2c/imx519.c` (Arducam Technology / Raspberry Pi Ltd) | **GPL-2.0-only** |
| `motors/ak7375/*` | Written for Daily Mirror | Apache-2.0 |

### The GPL-2.0 register tables

`sensors/imx519/private_include/imx519_settings.h` carries
`SPDX-License-Identifier: GPL-2.0-only`. Its mode and common register arrays are
copied from the Raspberry Pi kernel's IMX519 driver. **Do not relicense that
file, and do not strip its SPDX header.** It means this component as a whole is
distributed as `Apache-2.0 AND GPL-2.0-only`, and anything linking the P4 image
inherits the GPL-2.0 obligation for those tables. If that becomes a problem for
a shipping product, the tables have to be re-derived from the sensor datasheet
rather than copied.

## What was dropped from upstream

Upstream also ships IMX219, IMX708 and the DW9807 VCM. None of them are used by
Daily Mirror, so their sources, Kconfig blocks, CMake branches, IPA JSON configs
and README sections are not present here. The upstream repository remains the
place to get them.

## Traps worth knowing

- **The IPA JSON must be registered by the *project* CMakeLists**, between
  `include(project.cmake)` and `project()` — see `firmware/CMakeLists.txt`.
  esp_ipa reads `ESP_IPA_JSON_CONFIG_FILE_PATH` while its own CMakeLists is
  processed, which is too late for a component to set it. Get it wrong and
  nothing errors: esp_ipa's own check is spelled `message(FETAL_ERROR ...)`,
  which is not a real CMake mode, so it prints and carries on, and you get a
  stream with no auto-exposure and no white balance.
- **The VCM on the Arducam IMX519 module is an AK7375, not a DW9714/DW9807.**
  A DW97xx-style two-byte word write leaves the lens motionless and the focus
  sweep flat. The AK7375 wants `code << 4` as a 16-bit write to register 0x00,
  with 0x00/0x40 in register 0x02 for active/standby.
- **The sensor has no reset line**, so register state (e.g. the 0x0600 test
  pattern) survives a reboot. The driver clears it in `set_format`.
- `CAMERA_IMX519_DEBUG_LOG` logs every exposure/gain write. Off by default.
