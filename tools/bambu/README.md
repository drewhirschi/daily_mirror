# Bambu Lab A1 on the home network

The prototype housings in `hardware/` are printed on a Bambu Lab A1. Every step
here runs over the LAN: status, camera, file upload and starting prints. No
Bambu cloud account is needed.

## The printer

| | |
| --- | --- |
| Model | Bambu Lab A1 (serial prefix 039; the A1 mini is 030) |
| Serial | `03919D4C2802042` (CN of the printer's TLS certificate) |
| Address | `192.168.4.41` (DHCP; MAC `e8:06:90:a4:7a:54`, so reserve it on the router) |
| Bed | 256 x 256 x 256 mm |

Found 2026-10-16 by scanning for the printer's MQTT port 8883 and reading the
certificate, whose issuer is `BBL Technologies Co., Ltd / BBL CA`. The printer
did not answer Bambu's SSDP discovery broadcast (UDP 2021) during a 12-second
listen. If Bambu Studio can't find it, add it by IP.

## Ports it exposes

| Port | What | Login |
| --- | --- | --- |
| 8883 | MQTT over TLS: status reports on `device/<serial>/report`, commands on `device/<serial>/request` | `bblp` / access code |
| 990 | Implicit FTPS to the microSD card. Prints are uploaded here. Needs a card inserted | `bblp` / access code |
| 6000 | Camera: TLS, an 80-byte auth packet, then a 16-byte header plus a JPEG per frame | `bblp` / access code |

The printer uses a certificate from Bambu's private CA, so the tool skips
certificate verification.

## Setup

1. On the printer: **Settings → WLAN**, note the 8-digit **access code**.
   Turn on **LAN Only Liveview** if the camera refuses; leave cloud mode on if
   you still want the Bambu Handy app.
2. Create `tools/bambu/.env`. It is git-ignored; don't commit the code:

   ```
   BAMBU_HOST=192.168.4.41
   BAMBU_SERIAL=03919D4C2802042
   BAMBU_ACCESS_CODE=12345678
   ```
3. Run the tool with `uv`, which installs `paho-mqtt` for the script on first use:

   ```bash
   tools/bambu/a1.py status
   ```

## Commands

```bash
tools/bambu/a1.py status                 # temps, state, progress, card, filament
tools/bambu/a1.py watch --every 5        # keep printing status
tools/bambu/a1.py snapshot out.jpg       # one camera frame
tools/bambu/a1.py light on|off
tools/bambu/a1.py ls                     # files on the microSD card
tools/bambu/a1.py upload Housing.gcode.3mf
tools/bambu/a1.py print Housing.gcode.3mf --slot 1 [--no-level] [--timelapse]  # --slot: AMS Lite slot
tools/bambu/a1.py pause | resume | stop
tools/bambu/a1.py speed silent|standard|sport|ludicrous
tools/bambu/a1.py gcode "G28"            # raw G-code; the printer must be idle
tools/bambu/a1.py raw                    # full merged state JSON
tools/bambu/a1.py check --dir DIR        # timestamped frame + status; exit 1 if the printer reports an error
```

`print` takes a **sliced** `.gcode.3mf`, not an STL. Slice with `slice.py`
(below) or in the Bambu Studio GUI (**Export plate sliced file**), then upload and print.

## Printing a housing

STLs come out of each enclosure's `build.sh` into its `output/` folder (for
example `hardware/enclosure-p4/output/`). They're exported already oriented for
the bed. Starting settings for fit prototypes: Bambu PLA Basic, 0.20 mm
Standard, 4 walls, 15–20% infill, no supports. Print the fit coupons first.

## Safety

`gcode`, `print` and `stop` move or heat a real machine. Look at the camera
before starting a print remotely, and check the bed is clear. Don't leave a
first print unattended.

## Slicing without the GUI

`tools/bambu/slice.py OUT.gcode.3mf PART.stl [...]` runs Bambu Studio's CLI
with the stock A1 0.4 nozzle / 0.20 mm Standard / Bambu PLA Basic profiles
(4 walls, 20% infill, textured PEI plate by default; see `--help`). The CLI
does not follow the profiles' `inherits` chain, which silently produced a
200 mm bed and default speeds, so the script flattens them first. The CLI
can't render plate thumbnails (no OSMesa), so the printer's screen shows no
preview image; that doesn't affect printing.

## Watching a print

While a print runs, Claude takes a `check` every ~5 minutes, looks at the
frame (first-layer adhesion, spaghetti, part knocked loose, blobs on the
nozzle) and runs `stop` if it's clearly failing. Frames are kept in the
`--dir` folder.

## Firmware authorization (found 2026-10-16, A1 firmware 01.08.00.00)

Reading status, the camera, FTPS upload and the light work with just the
access code. **Starting, pausing or stopping a print over MQTT does not**: the
printer answers `project_file`/`stop` with `err_code 84033543` (0x05024007),
Bambu's "MQTT command verification failed". Since early 2025 Bambu's firmware
only accepts those commands from its own signed apps, unless the printer is in
**Developer Mode** (Settings → LAN Only Mode on → Developer Mode on). LAN-only
mode drops the cloud and the Bambu Handy app while it's on.

Without Developer Mode, start uploaded files from the touchscreen (SD card
file list) or from Bambu Studio. Claude can still watch, but can't cancel.
