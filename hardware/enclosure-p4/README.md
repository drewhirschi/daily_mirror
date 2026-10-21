# Daily Mirror — ESP32-P4-WIFI6 housing

Housing for the Waveshare **ESP32-P4-WIFI6** (71 x 21 mm Pico-style strip),
plugged into a **70 x 50 mm perfboard**, with the Arducam IMX519 on the front
panel. It mounts on the **enclosure-v2 rail and carriage, unchanged**: the four
M3 rear screws use the same pattern relative to the housing's bottom edge, so
print `RailSection` x N, `RailKey` x N-1, `Carriage`, `StopLower`, `StopUpper`,
`LockKnob` and `BrakePad` from `../enclosure-v2/output/`.

Outer size: 90 x 150 x 53 mm. Rebuild with `./build.sh` (FreeCAD 1.1).
Board dimensions come from Waveshare's STEP/DXF/drawing in `reference/`.

![preview](output/enclosure-preview.png)

## Layout

- Perfboard portrait on four 6 mm posts (M2 pilots), hole centres 2 mm in from
  each edge (46 x 66 mm). **Measure your board**; change
  `perfboard_hole_inset` in `dimensions.json` and rebuild if it differs.
- P4 plugs into two 1x20 female headers soldered 7 holes (17.78 mm) apart,
  USB-C end down, near the left of the perfboard (seen from the front). The
  USB-C plug points straight at the 14 mm bottom opening.
- The CSI connector is vertical. The 22-to-15 ribbon rises toward the front and
  turns up to the camera, whose connector faces down. About 10 mm of
  headroom is left above the P4.
- Front panel: 18 mm lens hole, 5 mm LED (upper right), 12 mm button (lower right).

## Print order (Bambu A1, PLA, 0.2 mm layers, 4 walls, 20% infill)

| STL | Orientation as exported | Notes |
| --- | --- | --- |
| `PerfboardFitCoupon` | flat | ~15 min. Screw the perfboard on to confirm the hole pattern |
| `CameraFitCoupon` | face down | ~10 min. Screw the camera on to confirm the lens hole and posts |
| `Housing` | rear on bed | No supports; the vents bridge 3 mm |
| `FrontPanel` | face down, posts up | No supports |

## Hardware

| Part | Qty | Use |
| --- | --- | --- |
| M2 x 6 thread-forming | 8 | Perfboard to posts, camera to front posts |
| M3 x 12 thread-forming | 4 | Front panel to housing |
| M3 x 10 thread-forming | 4 | Housing rear into carriage, from inside (fit **before** the perfboard) |
| 1x20 female headers | 2 | On the perfboard for the P4 |

Not checked: the IMX519's lens position (assumed to be 9.5 mm from the top edge, as on the Pi Cam v2), the exact perfboard hole positions, and the USB-C overmold width (13 mm max).
