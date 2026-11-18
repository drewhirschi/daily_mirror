# Daily Mirror — Raspberry Pi 4 tray and lid

> **Status: current.** This is the box to print. See [`../README.md`](../README.md) for the current build.

A closed version of the `../enclosure-p4-open` tray for the **Raspberry Pi 4
Model B**: the same 2.5 mm walls, 4 mm corners and screw ears, but with a full
lid. The **Arducam IMX519** hangs under the lid with its lens in a bore. Next to
it are a **12 mm tactile switch** (Omron B3F-4055) under a printed cap and a
**5 mm RGB status LED**, both on a small **control PCB**
([`../pcb-pi4-controls`](../pcb-pi4-controls)) that replaces the printed
ButtonCarrier. (Draft, 2026-11-16: the PCB and the lid changes for it are not
printed or ordered yet; see [Control PCB](#control-pcb-draft).)

Outer size: **65 x 124.4 x 37 mm** (76.6 mm wide across the lid screw ears).
Inside: 60 x 119.4 x 32 mm. The switch reaches **11.6 mm** behind the lid
(14.1 mm behind the lid's front face), terminal tips included.

The depth stays at 37 mm although the button no longer sets it. The camera
hangs 8.5 mm below the lid to its ribbon slot, and the ribbon has to pass over
the Pi's 16 mm USB stack; it clears it by 1.9 mm now. Only about 0.8 mm could
come off before that gap drops under 1 mm, and it rests on two numbers I have
not measured (Pi PCB thickness, the camera connector's slot height).

Rebuild with `./build.sh` (FreeCAD 1.1: build, verify, renders). Slice with
`./slice.sh` (Bambu A1, bed 55 C; it does not send anything to the printer).
Unlike the sibling folders, `output/` is committed here: STL, STEP, 3MF, renders
and the check reports.

![preview](output/enclosure-preview.png)

More views: [exterior](output/exterior.png), [cutaway](output/cutaway.png),
[exploded](output/exploded.png), [lid off](output/lid-off.png). Control PCB:
[lid posts](output/pcb-lid-posts.png), [PCB in the lid + stack](output/pcb-lid-assembled.png),
[front LED opening](output/pcb-front-led-opening.png).

## Layout

- Axes as in enclosure-p4-open: X across, Y along (microSD end at Y=0, camera
  end at Y=124.4), Z out from the flat back (Z=0). Hung on the wall, Y is up.
- The Pi stands **portrait, USB/Ethernet end up, components toward the lid**, on
  four 4 mm posts. Seen from the front, the USB-C/HDMI edge is on the right and
  the GPIO header on the left. The board drops straight in: 3 mm to the right
  wall (the audio jack sticks out 2.6), 1 mm on the left, 1.5 mm at the bottom.
- Above the Pi is a 27 mm bay with nothing on the floor. The camera, the LED
  and the button hang from the lid there, in a row at Y=105.75.
- **The lens is 15.5 mm right of centre** (X=48). The ribbon leaves the Pi's CSI
  socket 11.5 mm from the board edge and cannot shift sideways without a
  crease, so the camera sits on that line. The button is at X=18.5 and the LED at X=30.5.
- The back is a flat 65 x 124.4 face, full thickness everywhere. The only mark
  on it is a 16 mm finger scoop at the microSD end. **Rail mounting is not included yet.**

## Camera mount

- Four 5.3 mm posts on the lid's inside face, 21 x 12.5 mm pattern, 1.6 mm
  pilots. The camera screws on from behind with its lens side against the
  posts, so the board is parallel to the lid.
- The 7.5 mm lens barrel stands in an 8.5 mm bore (0.5 mm all round, for
  autofocus travel) and stops 1.8 mm below the lid face. The bore has a 0.8 mm
  chamfer outside so it does not clip the corners of the picture.
- A square collar under the lid wraps the top 1.2 mm of the 9.5 mm lens block
  with 0.4 mm clearance. Light from the Pi's LEDs has to pass that and the bore
  gap to reach the front.
- The connector edge points down at the Pi, so the picture is upright.

## Ribbon

The stock **150 mm** 15-pin ribbon is used as is. It rises from the CSI socket,
turns back toward the microSD end 11 mm above the board, makes a U-turn 3.3 mm
from the bottom wall, and runs straight back over the USB stack (1.9 mm above
it) into the camera connector. All three bends are **3.5 mm radius**, and the
ribbon stays flat across its width the whole way. See the cutaway render.

`fpc_length` in `dimensions.json` sets how far the fold-back goes. Lengths from
67.5 to 154.5 mm fit; a 100 mm ribbon folds back only 20 mm. Nothing printed
depends on the length.

## Openings

| Opening | Where | Size |
| --- | --- | --- |
| USB-C power | right wall, 15.2 mm from the bottom | 13.5 x 8 mm, for plugs up to 13 x 7 mm |
| micro-HDMI 0 and 1 | right wall, 30 and 43.5 mm from the bottom | 11.5 x 7 mm each, slim plugs only (11 x 6.5) |
| microSD | bottom wall, with a finger scoop in the back edge | 15 mm slot; the card ends 1 mm inside it |
| Vents | 6 slits in the left wall, 7 in the top wall, 4 in the bottom wall | 2.5 x 15 mm |
| Lens | lid | 8.5 mm bore |
| Button cap | lid | 10.6 mm, for the 10 mm printed cap |
| LED | lid | 5.5 mm through-hole with a 7.5 mm light collar behind it (was a 3.2 mm bore ending in a 2.2 mm window) |

- The power plug enters from the right side, 15 mm above the bottom, and sticks
  out about 30 mm. Use a right-angle plug if the cable should run down the wall.
- **USB-A, Ethernet and the audio jack have no openings.** The USB/Ethernet end
  faces the camera bay, not a wall, and this is a Wi-Fi device. A USB stick or
  short adapter still fits inside the bay.
- Set `hdmi_windows` to `false` for a closed wall.
- The top and bottom vents make a chimney when the housing hangs on the wall.

## Button and LED

**Switch: Omron B3F-4055** (now sold under the Aratas name), a 12 x 12 mm
through-hole tactile switch with a projected plunger, 2.55 N, SPST-NO.
From Omron's B3F datasheet, drawing on p.6
(<https://omronfs.omron.com/en_US/ecb/products/pdf/en-b3f.pdf>):

| Dimension | Value |
| --- | --- |
| Body | 12 x 12, 3.5 tall; 7.1 mm round base to 4.3 |
| Seating plane to plunger top | 7.3 ±0.2 |
| Plunger head | 3.8 mm square, 1.8 tall |
| Pretravel | 0.3 (+0.2/−0.1) |
| Terminals | 4, 1.0 x 0.3, 3.5 below the seating plane, 12.5 x 5.0 pattern |
| Locating bosses | two 1.6 mm, 9 apart |

Buy: [DigiKey](https://www.digikey.com/en/products/detail/omron-electronics-inc-emc-div/B3F-4055/31799).
I could not load DigiKey, Mouser or Newark to read a price. Findchips
(<https://www.findchips.com/search/B3F-4055>) showed Newark with 31,301 in
stock at $0.25 to $0.38 across quantity breaks, and Avnet and Sager in the same
range. Adafruit #1119 is a similar-looking 12 mm switch but names no part
number and is 6 mm tall, so it will not fit this stack.

How it goes together:

- **ButtonCap** (printed): a 10 mm stem through the lid, standing 1.2 mm proud,
  with a 13.5 mm flange under the lid that keeps it captive. A 4.4 mm square
  socket, 1 mm deep, sits on the plunger head. It has 0.3 mm of free play at
  rest and is still 0.9 mm proud when pressed.
- **ButtonCarrier** (printed): **superseded by the control PCB**, which has the
  same thickness, screw holes and switch holes. It is still built (and sliced
  into the existing plate) so the old assembly can be reproduced. A 17 x 25 x 1.6 mm plate under the switch. The
  four terminals pass through 2.0 x 2.2 mm slots and the two bosses through
  2 mm holes; two M2 x 6 screws hold it to 8.1 mm posts on the lid. The
  terminal tails stand 1.9 mm below the plate for soldering.
- Order: drop the cap into the lid from inside, set the switch on it, screw the
  carrier (now the control PCB, switch soldered in) on.

**LED** (was a 3 mm Kingbright WP710A10SGD in a 3.2 mm bore with a 2.2 mm
window): now the EBOOT 5 mm RGB LED on the control PCB, below.

## Control PCB (draft)

A 30 x 27.1 x 1.6 mm board ([`../pcb-pi4-controls`](../pcb-pi4-controls)) takes
the ButtonCarrier's place and widens toward the left wall to carry the LED, J1
and three resistors. The switch, cap and the two existing posts are unchanged:
the board is the carrier's thickness, has the carrier's two 2.4 mm screw holes
at (18.5, 96.25) and (18.5, 115.25), the carrier's 2.0 x 2.2 mm terminal slots
and 2.0 mm boss holes, and its top face is the switch seating plane (Z 26.4).

- **Outline** X 4.0 to 34.0, Y 93.25 to 120.35 (`pcb_outline`). 1.5 mm to the
  left and top walls, 1.5 mm to the camera board edge in plan (1.9 mm in 3D;
  the camera board is 1.1 mm higher). The bottom edge is where the carrier's
  was, 1.25 mm past the USB stack face.
- **LED posts (new)**: (30.5, 96.25) and (30.5, 115.25), the same 4.4 mm x
  8.1 mm posts with 1.6 mm pilots as the button posts, in line with them. They
  clear the camera posts by 3.0 mm and the camera board by 2.8 mm; nothing had
  to move. Four M2 x 6 screws hold the board.
- **LED opening (new)**: a plain 5.5 mm through-hole at (30.5, 105.75) and a
  7.5 mm collar from the lid underside down to Z 30.8, 0.5 mm above the LED
  flange. The collar keeps the LED's side glow away from the lens bore; it
  clears the button cap flange by 1.5 mm. The old 3.2 mm bore, 2.2 mm window
  and 6 mm collar are gone.
- **LED height**: the 5 mm LED is 7.7 mm from flange underside to tip, so its
  **flange stands 2.9 mm above the board** for the tip to sit flush with the
  lid face (Z 37). Easiest: push the LED through the board, screw the board in,
  press the dome flush with the face, then solder.
- **J1** is a straight 1x6 header on the back near the left wall, pins toward
  the tray floor. A straight six-wire ribbon with a 1x6 housing at each end goes
  to the Pi's inner header column, pins 1 to 11 (pinout and build steps in
  `../pcb-pi4-controls/ASSEMBLY.md`). With a Dupont housing on it the housing ends 5.3 mm above the
  floor (Z 7.76); the model reserves another 4 mm under it for the wires to
  turn. The housing is 2.55 mm from the Ethernet jack face.
- **R1-R3** lie flat on the back above J1; their trimmed lead tails on the front are 1.15 mm from the switch body.

The checks for all of this are in `verify.py` (search "Control PCB").

## Raspberry Pi 5

The Pi 5 has the same outline, mounting holes and USB-C position, so it screws
onto the same posts and takes power through the same window. I did not model it,
and the Lid needs changes before it can be used:

| Item | Pi 5 | Effect |
| --- | --- | --- |
| micro-HDMI | 0.2 and 0.3 mm closer to USB-C | Fits the same windows, with 0.05 mm less margin on one side |
| Camera sockets | two 22-pin, about 2.3 and 8.5 mm further up, 3 mm closer to the board edge | Needs a 22-to-15 ribbon, and the camera posts, bore and collar move about 3 mm right. Change `CSI` in `build.py` |
| USB and Ethernet | swapped | None; both are enclosed |
| Power button | on the microSD edge | Not reachable |
| Audio jack | none | None |
| Active cooler | about 10 mm tall | Fits; there is 26.5 mm above the board |

## Parameters to measure

All in `dimensions.json`. Pi positions come from the official mechanical
drawing; the ones below are scaled from it or assumed.

| Key | Now | What it is |
| --- | --- | --- |
| `pi_pcb_thickness` | 1.5 | Not on the drawing; 1.4 to 1.6 |
| `pi_underside_allowance` | 2.5 | Pin tails and the microSD socket under the board (posts are 4) |
| `pi_port_side_gap` | 3.0 | Must exceed the audio jack overhang (2.6, scaled) |
| `usbc_window_width`, `usbc_window_height` | 13.5, 8 | Your plug's overmold plus 0.5 |
| `hdmi_window_width`, `hdmi_window_height` | 11.5, 7 | Same, for micro-HDMI |
| `camera_lens_from_connector_edge` | 9.75 | From Arducam's STEP; the Pi Camera v2 is 9.46 |
| `camera_lens_block_height` | 5.0 | Sets the post height (block + 0.3) |
| `camera_connector_slot_below_pcb` | 1.5 | Where the ribbon enters the camera connector; sets the ribbon height |
| `fpc_length` | 150 | Your ribbon |
| `cap_hole`, `cap_diameter` | 10.6, 10.0 | 0.3 mm radial; open the hole up if the cap sticks |
| `cap_rest_gap` | 0.3 | Free play of the cap; the switch height is ±0.2 |
| `cap_socket_clearance` | 0.3 | Around the 3.8 mm plunger head |
| `carrier_slot` | 2.0 x 2.2 | Terminal span is 12.5 ±0.5 at the tips |
| `led_height` | 7.7 | LED flange underside to tip; sets the 2.9 mm standoff |
| `pcb_outline` | 4, 93.25, 34, 120.35 | Control PCB outline; the KiCad board reads it |

Three things I could not confirm from documents and that the layout depends on:

- **Ribbon contact sides.** The route assumes the contacts face the HDMI ports
  at the Pi and face the camera's PCB at the camera, as on the Pi Camera v2.
  Arducam's documents disagree with each other on the camera end. If the
  camera's contacts face away from its PCB, this route does not work.
- **Switch details the drawing leaves out:** total travel past the 0.3 mm
  pretravel, the boss length (the carrier holes go right through, so any length
  works) and the plunger neck width.
- **microSD slot position** along the board edge (28 mm from the USB-C edge)
  and the card overhang (2.5 mm). The slot is 15 mm wide for an 11 mm card.

## Print order (Bambu A1, PLA, 0.2 mm, 4 walls, 20% infill, bed 55 C)

| File | Orientation as exported | Time |
| --- | --- | --- |
| `PiFitCoupon` | back on bed | ~30 min, 16 g. Bottom 15 mm of the Tray: Pi posts, USB-C and HDMI windows, microSD slot. Print it **first** |
| `LidFitCoupon` | outer face on bed | ~18 min, 6 g (not re-sliced since the LED change). Camera end of the Lid: camera posts, bore, collar, cap hole, switch posts, LED posts and hole |
| `ButtonCap` | flange on bed | ~6 min, 1 g |
| `ButtonCarrier` | flat | ~6 min, 1 g. Superseded by the control PCB |
| `Tray` | back on bed | ~1.7 h, 60 g |
| `Lid` | outer face on bed, posts up | ~36 min, 22 g |

`output/Lid-v2.gcode.3mf` is the current Lid sliced for the Bambu A1, the exact
job printed on 2026-11-16. `output/TrayAndLid.3mf` is the Tray, Lid, ButtonCap
and ButtonCarrier as a plain model file. The old one-plate
`TrayAndLid.gcode.3mf` held the previous Lid (3 mm LED bore, no board posts)
and was removed so it cannot be printed by mistake.

No supports are needed. The port windows bridge up to 13.5 mm, the wall over
the microSD slot bridges 16 mm, and the vents bridge 2.5 mm. The lid prints
with its front face on the plate, so the front takes the plate's texture.

## Screws and parts

| Part | Qty | Use |
| --- | --- | --- |
| M2.5 x 6 thread-forming | 4 | Pi to tray posts (`pi_pilot` 2.1, 5.5 deep) |
| M2 x 6 thread-forming | 8 | Camera (4) and control PCB (4) to lid posts (`camera_pilot` 1.6) |
| Omron B3F-4055 | 1 | Button |
| Control PCB with EBOOT 5 mm RGB LED, 3 x 330 ohm, 1x6 header | 1 | Status light and button board, see `../pcb-pi4-controls` |
| M3 x 10 thread-forming | 6 | Lid to tray ears (`case_pilot` 2.5; 3.4 clearance in the lid) |
| 15-pin 1.0 mm camera ribbon, 150 mm | 1 | The one supplied with the camera |

For M2.5 heat-set inserts in the Pi posts, set `pi_pilot` to the insert's hole
(about 3.4) and `pi_post_diameter` to 7. I have not checked that a 7 mm post
clears the parts under the board.

## Files

- `output/Tray.step|stl`, `output/Lid.step|stl`, `output/ButtonCap.step|stl`, `output/ButtonCarrier.step|stl`
- `output/PiFitCoupon.step|stl`, `output/LidFitCoupon.step|stl`
- `output/TrayAndLid.3mf`, `output/Lid-v2.gcode.3mf`, `output/Assembly.step`
- `output/enclosure-preview.png`, `exterior.png`, `cutaway.png`, `exploded.png`, `lid-off.png`
- `output/pcb-lid-posts.png`, `pcb-lid-assembled.png`, `pcb-front-led-opening.png` (control PCB change)
- `output/verify.json` (128 fit checks), `output/validation.json`, `output/layout.json`, `output/manifest.json`
- `output/DailyMirror-Pi4.FCStd` is rebuilt by `./build.sh` and not committed.

The Pi, camera, ribbon, switch, LED, control PCB, header and Dupont housing in the renders and checks are stand-ins I
modelled from the drawings, not vendor STEP files. Nothing has been printed or
test-fitted.
