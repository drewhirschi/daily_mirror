# rpi2 replacement IMX519 calibration

Verified September 10, 2026 (Denver). The replacement module's brand/model was
not supplied. Sensor detection identifies IMX519 at I2C 10-001a and the AK7375
lens driver at 10-000c. Full-resolution capture is 4656 × 3496.

## Installed changes

- Replaced the obsolete OV5647 boot overlay with `dtoverlay=imx519`, leaving
  automatic camera detection disabled. Rebooted and verified sensor detection.
- Set `DAILY_MIRROR_CAMERA_PROFILE=imx519`. Capture mode remains local.
- Installed the device binary with standby and processing LEDs off. Successful
  normal capture flashes green twice, then goes dark. Countdown yellow and
  error red remain. The admin lamp is labeled Success.
- Stock IMX519 tuning had no `rpi.af`; logs explicitly reported no AF algorithm.
  The existing AK7375 driver did expose focus controls. Added a separate
  `/home/drew/daily-mirror-device/config/imx519-af.json`, selected through
  `LIBCAMERA_RPI_TUNING_FILE` in the device `.env`. Stock package files are intact.
- `scripts/prepare_imx519_tuning.py OUTPUT` recreates the tuning candidate from
  installed IMX519 calibration and installed autofocus settings. PDAF is disabled
  in favor of contrast autofocus. Its actuator map is provisional, based on the
  [IMX519 upstream proposal](https://lists.libcamera.org/pipermail/libcamera-devel/2023-June/038295.html).
  Lens-position numbers do not yet represent calibrated physical distances.

The old boot config and environment are backed up beside their originals with
`.before-imx519` suffixes. The previous device binary is kept in `bin/`.

## Screen calibration

`scripts/calibration_target.py` opens a Tk window. Write `chart`, `text`, `gray`,
or `image:/absolute/path` to `/tmp/mirror-target-mode` to change its contents.
Escape closes the window. Requires tkinter and Pillow.

Far-right display: portrait HP E243, HDMI-A-1, workspace 5, monitor ID 2.
This Hyprland version uses Lua dispatch; after discovering the current window
address with `hyprctl clients -j`, use:

```sh
hyprctl dispatch 'hl.dsp.window.move({workspace = "5", follow = false, window = "address:CURRENT_ADDRESS"})'
```

Chart settings are in `evidence/imx519-rpi2/screen-lab-settings.json`. Apply with
`POST /api/lab/settings` using a JSON body. These are screen-test settings:
1/60 second shutter, gain 3, default color processing, continuous focus and a
focus region covering the monitor. They are not general portrait exposure
settings. The lab retains them in memory; only orientation persists through the
lab settings endpoint. Normal capture retains automatic exposure and the normal
portrait focus window, using the newly installed AF tuning.

Observed chart metadata: AfState 2 (focused), LensPosition 6.607428,
ExposureTime 16662 microseconds, AnalogueGain 2.994152. Text and grayscale steps
were visibly resolved. Camera placement changed during setup, so raw sharpness
scores across those frames are not a controlled comparison. Color accuracy is
not certified by a photograph of an uncalibrated display.

## Validation

- 20 unit tests and one integration test pass; device Clippy passes with warnings
  denied. The release cross-build succeeded and deployed health check passed.
- Normal local capture completed. Live LED events showed both success flashes,
  followed by ready with every logical output off. Recorded in
  `evidence/imx519-rpi2/led-events.json`.
- Full-resolution chart, text, and image-target lab captures completed without gallery upload.
- Boot sensor selection and service tuning survive a service restart; the boot
  overlay itself was verified by reboot.

## Portrait rendering follow-up

A later pass moved the camera farther from the chart and compared eight lab/
direct captures. No person was in those frames, so skin tone, facial texture,
and subject-motion quality are still unverified.

Selected starting preset:

- Separate tuning file `config/imx519-portrait.json` retains the AF fix and
  gently lifts the stock output gamma curve (`y → y^0.85`, normalized 0–1).
  Black and white endpoints remain unchanged. This is single-frame tone tuning,
  not HDR fusion or calibrated color processing.
- Saturation 1.15 and sharpening 1.15. Contrast 1, brightness 0, EV 0.
- Automatic white balance and gain, `sport` exposure, and automatic denoising.
- Continuous fast autofocus with the standard central portrait window.

The brighter +0.3 EV / contrast 1.08 variant washed out more of the bright chart.
The selected tone lift opens shadows without that extra exposure. In the
controlled tone pair, shutter was 19983 µs with gain about 4.6 and AfState 2 in
both frames. Baseline normal exposure was 29944 µs / gain about 3.1. Shorter
shutter reduces motion-blur risk but the static chart cannot quantify that gain.

Forced `cdn_hq` during the whole camera run caused two three-second captures to
report AfState 1 (searching), with visibly soft text. Returning to `auto`
produced three focused repeats. Raspberry Pi's [camera documentation](https://www.raspberrypi.com/documentation/computers/camera_software.html)
notes that forced high-quality denoising reduces viewfinder frame rate; auto
already selects high-quality color denoising for still images. Do not force
`cdn_hq` as a blanket quality upgrade.

The preset is saved for normal button/admin captures in the device `.env`.
`DAILY_MIRROR_CAMERA_ARGS` must be quoted because it contains spaces; an unquoted
trial was not applied by the environment loader and was corrected before final
verification. Previous settings are backed up as `.env.before-portrait-tuning`.
The lab uses `evidence/imx519-rpi2/portrait-lab-settings.json` with the same tuning
file. Its controls remain session settings; normal capture uses the saved args.

Recreate the full tuning file on the Pi with:

```sh
python3 scripts/prepare_imx519_tuning.py --shadow-exponent 0.85 /tmp/imx519-portrait.json
```

The script reproduced the installed tuning file byte for byte. Comparison
metadata is saved in `evidence/imx519-rpi2/portrait-comparison.json`.
