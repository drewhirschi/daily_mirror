# Next enclosure: Pi 4 B and one LED

Planning baseline: 2026-09-05. This supersedes the RGB-ring direction for the
next prototype in the older hardware plan. This is a build plan, not a finished
CAD model or verified wiring diagram.

## Decisions and missing measurements

- Use Raspberry Pi 4 Model B for the next version.
- Print a housing that securely holds both the Pi and camera.
- Use one LED, probably white. Assume a status indicator pending confirmation;
  photographic illumination would need a different electrical and optical design.
- Retain the capture button. Confirm its new part and panel cutout.
- Existing project documentation identifies an IMX519 camera; confirm whether
  the new parts include a different camera or board revision.
- Obtain LED part number/datasheet, camera board dimensions and hole spacing,
  lens protrusion, ribbon length, button dimensions, installed cooler dimensions,
  intended wall/desk mounting, and printer model before freezing geometry.

## Linux tools

Recommended: [FreeCAD](https://www.freecad.org/) for dimensioned sketches,
extrusions, screw holes, standoffs, and editable mechanical parts. Its parametric
design lets us change dimensions without rebuilding everything. Start with a
stable Linux release from the [official downloads](https://www.freecad.org/downloads.php).

Other established options:

| Tool | Fit for this project |
| --- | --- |
| [OpenSCAD](https://openscad.org/about.html) | Free, Linux-native, code-based solid modeling; useful for version-controlled, parameterized enclosures. Best alternative if scripting feels more natural than sketching. |
| [Onshape](https://www.onshape.com/en/pricing) | Browser-based CAD; an option on Linux without a desktop install. The free plan is for noncommercial work and makes documents public. |
| [Blender](https://www.blender.org/features/) | Linux support and extensive modeling tools; better suited to sculpted shapes and visualization than our initial dimensioned enclosure. |

Use the printer's supported slicer to turn exported geometry into print jobs.
[PrusaSlicer](https://help.prusa3d.com/article/install-prusaslicer_1903) is a
Linux option if it supports the selected printer. Keep editable CAD source as
well as exported STL/3MF files; the slicer does not replace CAD.

## Build sequence

1. **Measure and bench-test the new parts.** Record a small BOM and wiring
   sketch. Prove the camera, button, and single LED before enclosing them.
2. **Model and print only the mounting pieces.** Start with a Pi tray and
   separate camera bracket. Use the official
   [Pi 4 mechanical drawings](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#schematics-and-mechanical-drawings)
   and physical measurements of the actual camera. Check screws, clearances,
   lens position, ribbon routing, and cooler space with real parts.
3. **Add a simple shell and screwed lid.** Provide a camera opening, LED
   holder, button opening, USB-C access, ventilation, and service access to
   connectors/SD card. Leave space for plugged-in cables, their bends, and the
   GPIO harness. Keep the camera fixed when removing the lid. Use a separate
   opaque baffle between LED and lens; test that it does not clip the image.
4. **Print and assemble the first complete enclosure.** Start with simple,
   easy-to-print geometry. Try roughly 2–3 mm walls and a small clearance test
   coupon before committing lid fits; final values depend on printer/material.
   PLA can serve for an initial fit check; choose the installed enclosure
   material after checking temperatures and its temperature ratings.
5. **Validate in its intended location.** Check framing, focus, LED reflections,
   button usability, Wi-Fi, and reliable captures/uploads. Measure temperature
   and throttling during an enclosed 24-hour run with repeated captures.
   Revise ventilation/cooling if necessary. Confirm opening and reassembly do
   not move the camera or pull on its ribbon.

First milestone: **a printed tray and camera bracket that fit the actual parts**.
The outer styling can follow once component placement works.

## Single-LED wiring and software work

The current device program allocates three outputs: BCM 17, 27, and 22.
Plan to reuse BCM 17 for the single indicator, but implement an explicit
single-LED mode first. Do not configure all three existing outputs to one pin.
Update the admin status display and LED test controls with the new mode.

For a bare, low-current indicator LED, the candidate circuit is:

```text
BCM 17 output -> series current-limiting resistor -> LED anode
LED cathode -> Pi GND
```

Confirm physical header pin numbers against the Pi pinout when producing the
final wiring sheet. Select resistance from the LED datasheet and chosen current:
R = (supply voltage - forward voltage) / current. White LEDs may have little
voltage headroom on a 3.3 V GPIO output; do not assume a resistor value or that
direct drive gives sufficient brightness. If necessary, use a transistor/MOSFET
switch with an appropriately current-limited supply, common ground, and a default
off bias. Keep 5 V away from GPIO. A higher-power photographic LED needs a
suitable driver and thermal design, not direct GPIO power. See the official
[GPIO hardware guidance](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html).

Proposed status patterns to test for clarity:

- Ready: steady light at a comfortable brightness.
- Countdown: distinct pulses, speeding up toward capture.
- Exposure: off to avoid reflections.
- Uploading: slow blink; successful upload: two quick flashes, then ready.
- Error: repeating groups of three flashes; details in the admin page.

Verify boot, capture, queue/retry, error, and manual LED-test behavior on the
bench. Then solder the proven circuit onto a small secured perfboard with
removable connectors and strain relief. Final resistor/driver selection and
panel-hole size remain pending the exact LED part.

## First CAD concept — 2026-09-05

The [enclosure model and build instructions](../hardware/enclosure/README.md)
now provide a roomy 180 × 180 × 73 mm box with separate wiring reserve,
replaceable camera plate, and a future 300 mm height-adjustment study.
FreeCAD 1.1.3 Python/headless generation and CAD/mesh round trips were verified.
Camera mounting dimensions and LED/button openings still require actual parts.
The rail is a motion study, not an assembly-ready print; see the model README
for whole-enclosure versus camera-only movement and mounting tradeoffs.
