# Hardware index

Read this before printing, ordering or editing anything under `hardware/`.
It says which design is the current one. Everything else is kept for
reference and must not be printed or built from by default.

## Current build

**The box is the Raspberry Pi 4 tray and lid in [`enclosure-pi4`](enclosure-pi4).**
Its button and status LED sit on the board in [`pcb-pi4-controls`](pcb-pi4-controls).

| Part | Source | Print or fab file | Last made |
| --- | --- | --- | --- |
| Tray | `enclosure-pi4/output/Tray.stl` | slice from the STL | printed 2026-11-12 |
| Lid (v2: 5 mm LED opening, four board posts) | `enclosure-pi4/output/Lid.stl` | `enclosure-pi4/output/Lid-v2.gcode.3mf` (Bambu A1, the exact job that printed) | printed 2026-11-16 |
| ButtonCap | `enclosure-pi4/output/ButtonCap.stl` | slice from the STL | printed 2026-11-12 |
| Control board v0.4 | `pcb-pi4-controls/pi4-controls.kicad_pcb` | `pcb-pi4-controls/output/fab/` | **not ordered yet** |

The printed `ButtonCarrier` is replaced by the control board; do not print it.

## Status of every folder

| Folder | Status | What it is |
| --- | --- | --- |
| `enclosure-pi4` | **CURRENT** | Pi 4 tray and lid with the IMX519 camera |
| `pcb-pi4-controls` | **CURRENT** | Button and RGB LED board for the Pi 4 lid |
| `enclosure-p4-open` | Other device | Open tray for the ESP32-P4. Last sliced 2026-10-20. Not the current box |
| `enclosure-p4` | Older | Closed ESP32-P4 housing from before `enclosure-p4-open` |
| `enclosure-v2` | Superseded | Rail enclosure prototype 02 for the Pi 4 (2026-09-10). The box is replaced by `enclosure-pi4`; `enclosure-p4` still refers to its rail and carriage |
| `enclosure` | Superseded | Enclosure concept 01 |
| `pcb-p4` | Study | ESP32-P4 board component study, never a finished board |
| `prototype-modular` | Notes | Parts comparison from 2026-09-14 |
| `wiring` | Reference | Wiring drawings for the P4 and S3 |

## Keeping this true

- When a design replaces another, change the Status here and the banner at the
  top of the old folder's README in the same commit.
- When a part is printed or a board is ordered, update "Last made" above.
- Name a changed part with a version (`Lid-v2`) so an old print file cannot be
  mistaken for the new one.
