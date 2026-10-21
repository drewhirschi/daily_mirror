# Daily Mirror — ESP32-P4 open tray

A low, open-top tray for the Waveshare **ESP32-P4-WIFI6** plugged into a
**50 x 70 mm perfboard**, with a short screwed-on **CameraLid** over the top
end of the board that carries the Arducam IMX519. The button and RGB LED on the
perfboard stay reachable through the open top. Same stack as `../enclosure-p4`
(6 mm posts, 8.5 mm female headers, 2.5 mm male-header spacer).

Outer size: **56 x 78.5 x 33 mm** (67.6 mm wide across the lid screw ears).
The lid is **23 mm** long and covers the top **20 mm** of the board. The camera
stands on the lid, so the lens top is 44.6 mm above the back.

Rebuild with `./build.sh` (FreeCAD 1.1: build, verify, assembly, preview). Slice with
`./slice.sh` (Bambu A1, bed 55 C, writes the git-ignored `sliced/`; it does not
send anything to the printer). Open the assembly in desktop FreeCAD with
`freecad OpenAssembly.FCMacro`. The first open imports the Waveshare STEP from
`../enclosure-p4/reference/` and takes a while.

![preview](output/enclosure-preview.png)

## Layout

- Axes as in enclosure-p4: X across, Y along the board (USB-C end at Y=0, lid
  end at Y=78.5), Z out from the flat back (Z=0).
- Perfboard on four 6 mm posts with M2 pilots, 0.5 mm clearance to the walls.
  There is 3 mm at the USB end because the 71 mm P4 is longer than the 70 mm
  board and its USB-C shell sticks out another 1.35 mm.
- The P4 is 2.5 mm in from the perfboard's left edge, with its antenna/module end
  flush with the lid end of the perfboard. USB-C points at a 14 mm U-notch in
  the bottom wall. The notch is open at the top, so the plug drops in from above.
- The lid plane (tray rim) is at Z=30.5. The tallest P4 part is at 27.4 (3.1 mm under
  it). The LED can stand up to **20.4 mm** above the perfboard; the allowance is 19.
- **The camera is on the outside of the lid, lens out.** I checked mounting it
  inside the lid with the real STEP (`verify.py`, `output/verify.json`). Pressed
  against the lid's underside, the camera's ribbon connector would sit at Z=26.4,
  1 mm *into* the P4's 4.7 mm part under it. That part is at Y 19–23 from the
  module end. The ribbon would also have no room to turn down into the P4's
  vertical CSI socket, whose top is at 26.8. An inside mount needs about 37.5 mm
  of overall height, not 33. On the outside, the camera stands on 4 mm posts that
  clear its back connector. The lid stops 2.5 mm short of the camera's connector
  edge, so the 16 mm FPC drops straight past the lid edge into the open tray, to the
  CSI connector about 12 mm away.
- The lid is held by four M3 screws into full-height ears on the tray's side
  walls. Nothing inside the tray can stick out over the perfboard, or the board
  would not drop in from the top.
- **Rail mounting is not included yet.** The back is a flat 56 x 78.5 face with
  nothing on it, so a carriage plate or screw bosses can be added later.

## Parameters to measure

All in `dimensions.json`. Perfboard coordinates are from its corner at the USB
end, on the left as seen from the open front.

| Key | Now | What it is |
| --- | --- | --- |
| `perfboard_hole_inset_x` | 2.0 | Hole centre from the long edges (guess) |
| `perfboard_hole_inset_y` | 2.0 | Hole centre from the short edges (guess) |
| `perfboard_holes` | null | Optional `[[x,y],...]` for all four holes; replaces the insets |
| `p4_offset_x` | 2.5 | Perfboard left edge to P4 left edge |
| `p4_end_inset` | 0.0 | P4 module end back from the perfboard's lid-end edge (max 0.5 or it hits the wall) |
| `led_x`, `led_y`, `led_height` | 38, 34, 19 | LED centre and top above the perfboard (max 20.4) |
| `button_x`, `button_y`, `button_height` | 38, 14, 12 | Button centre and top above the perfboard |
| `camera_lens_from_top_edge` | 9.5 | IMX519 lens position, assumed from the Pi Cam |
| `total_height` | 33.0 | Raise it if anything grows |

Current hole centres (from the perfboard corner): (2, 2), (48, 2), (2, 68), (48, 68).
That is a 46 x 66 mm pattern.

## Print order (Bambu A1, PLA, 0.2 mm, 4 walls, 20% infill, bed 55 C)

| File | Orientation as exported | Time |
| --- | --- | --- |
| `PerfboardFitCoupon` | flat | ~12 min, 6 g. Screw the perfboard on to check the holes **first** |
| `CameraLid` | underside on bed, posts up | ~12 min, 4 g |
| `Tray` | back on bed | ~55 min, 37 g |

No supports are needed.

## Screws and parts

| Part | Qty | Use |
| --- | --- | --- |
| M2 x 6 thread-forming | 4 | Perfboard to tray posts (`perfboard_pilot` 1.6) |
| M2 x 6 thread-forming | 4 | Camera to lid posts (`camera_pilot` 1.6) |
| M3 x 10 thread-forming | 4 | Lid to tray ears (`case_pilot` 2.5; 3.4 clearance in the lid) |
| 1x20 female headers, 8.5 mm | 2 | On the perfboard for the P4, 17.78 mm apart |
| 15-pin camera FPC (22-to-15 for the P4) | 1 | Short, about 50 mm; the run is about 12 mm across and 10 mm up |

## Files

- `output/Tray.step|stl`, `output/CameraLid.step|stl`, `output/PerfboardFitCoupon.step|stl`
- `output/FullAssembly.FCStd` (opened by `OpenAssembly.FCMacro`), `output/DailyMirror-P4-Open.FCStd`
- `output/validation.json` (envelope checks), `output/verify.json` (STEP checks), `output/layout.json`

Not checked: perfboard hole positions, LED and button parts and positions, the
IMX519 lens position, the USB-C overmold width (13 mm max), and the ribbon length.
