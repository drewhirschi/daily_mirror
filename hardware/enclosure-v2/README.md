# Daily Mirror - rail enclosure prototype 02

This is the September 10, 2026 revision for the **confirmed Raspberry Pi 4 Model B**.
It replaces the old rail motion study with printable mating parts, a thumb lock,
removable end stops, actual camera standoffs, and front control holes.
The earlier `hardware/enclosure` concept remains available unchanged.

**Start with `output/model-viewer.html` in a browser** to rotate the real model,
show the internals, separate the lid, and move it through its full travel.
FreeCAD's window is not needed for viewing this preview or printing the STLs.

For the mounting details, select **How the Pi screws in** or **How the rail
attaches** in the viewer. Both views separate the parts and show reference
screws; move separation to zero to see them assembled. The attachment view
shows only a short section of the actual rail for clarity. The four enclosure
screws are inserted from inside the case into the orange sliding block.
The Pi uses its four existing mounting holes; additional retaining clips are
not part of this screw-mounted design. Reference fasteners are not printed.

For scale, the enclosure is about **5.5 x 6.7 x 2.2 inches**, versus an
85 x 56 mm Pi PCB. It deliberately includes room for the camera, controls,
power-plug routing, cooling and later flash components. It is smaller than
the earlier 180 x 180 x 73 mm enclosure concept.

![Enclosure, internal mounts and rail](output/enclosure-preview.png)

## What is modeled

| Feature | Prototype dimensions / behavior |
| --- | --- |
| Enclosure | 140 wide x 170 tall x 55 deep mm including front panel; 3 mm walls; rounded corners |
| Pi mounts | Four 8 mm raised posts, 58 x 49 mm hole centers; 2.0 mm pilot bores |
| Camera mounts | Four 9 mm raised posts on the front panel; 21 x 12.5 mm hole centers; 1.6 mm pilots |
| Front openings | 18 mm lens, 5.2 mm status LED, 12.2 mm button |
| Power | One 14 mm bottom cable passage, internal zip-tie saddles; no exposed USB/Ethernet/HDMI ports |
| Rail | Two 208 mm sections, 416 mm assembled; 80 mm wide, 12 mm maximum wall depth |
| Rail joint | 6 mm rear alignment tongue; each section independently screws to the wall |
| Carriage | 60 x 100 mm, captive dovetail, 0.4 mm horizontal side and top allowances |
| Adjustment | 300 mm continuous travel between separately installed end stops |
| Lock | M4 thumb screw, captive nut, 8 mm rubber/TPU brake pad; knob accessible below the box |
| Wall projection | 78 mm nominal to front face, excluding button/lens projection |
| Future flash | 24 x 28 x 20 mm reserved volume; replaceable front panel for later optics |

The **entire box moves**: Pi, camera, LED, button and their wiring. Only the
external power cable flexes during height adjustment. Leave a loose service loop
that reaches both travel limits without tugging on the plug.

This design assumes a **new printed rail**, not compatibility with a purchased
rail. It uses wall screws, not adhesive strips. Each rail section must be fixed
to the wall; the small tongue aligns the joint and does not carry the wall load.

## Files and slicer basics

- `output/DailyMirror-Rail-Enclosure-v2.FCStd`: native FreeCAD document with
  named solids, grouped printable parts, hardware references and fit coupons.
- `OpenModel.FCMacro`: optional FreeCAD macro that opens the document with
  clean visibility and part colors. Run via Macro > Macros, or open the macro
  file in FreeCAD. The model can be opened directly without running the macro.
- `output/*.stl`: **one physical part per file**, in millimeters, pre-oriented
  and moved onto the print bed. Import each as an independent part at 100% scale.
- `output/*.step`: CAD solids in **assembly coordinates**, useful for editing
  or moving to another CAD program. `Assembly.step` has the nine print parts.
- `output/model-viewer.html`: self-contained interactive preview; no account,
  external library, network, or FreeCAD install needed. It requires WebGL.
- `output/enclosure-preview.png` and `output/mount-details.png`: still previews.
- `dimensions.json` and `build.py`: the coordinated dimension source and
  modeling recipe. Change the JSON and rebuild to change the fit. The FCStd
  contains editable solids, **not a fully constrained sketch-based feature tree**.
- `output/validation.json`: actual geometry/interference check results.

An STL describes the shape. Your **slicer** combines it with your printer,
nozzle and material settings to produce the printer's job file, often G-code.
No printer-specific job has been generated because the printer is not yet known.
Do not send STEP/FCStd files directly to the printer.

## Print these small pieces first

1. `RailFitMale.stl` and `RailFitFemale.stl`: matching short sections of the
   exact dovetail. They should slide by hand without being forced and without
   excessive rocking. Remove elephant's foot first. Change `rail_clearance`
   in 0.1 mm increments and rebuild if needed; do not globally scale the parts.
2. `CameraFitCoupon.stl`: a small section of the actual front panel, containing
   the real lens opening and four real posts. Screw the camera onto it from
   behind. Confirm hole positions, screw engagement, component clearance and
   full-field image coverage. The camera faces through the hole; its ribbon
   connector points toward the bottom of the enclosure.
3. Check a Pi screw gently in one post and confirm your M4 nut and bolt head fit
   the carriage and knob pockets before assembling everything.

## Full print list

| STL | Quantity | Supplied print orientation | Notes |
| --- | --- | --- | --- |
| Housing | 1 | Flat rear on bed, cavity up | Vents bridge 3 mm; tie saddles bridge 3 mm |
| FrontPanel | 1 | Exterior face on bed, camera posts up | Keep lens hole and posts clean |
| RailLower | 1 | Flat back on bed | 80 x 214 mm footprint including alignment tongue |
| RailUpper | 1 | Flat back on bed | 80 x 208 mm; small rear key pocket bridges 16.5 mm |
| Carriage | 1 | Standing on short end | 60 x 18.6 mm footprint, 100 mm tall; use brim |
| StopLower / StopUpper | 1 each | Standing on short end | These are separately installed pieces |
| LockKnob | 1 | Flat back on bed, hex pocket up | Captures M4 hex-head screw |
| BrakePad | 1 | Flat | TPU, or cut an 8 mm disc from 1.5 mm rubber |

Starting settings for a fit prototype: 0.4 mm nozzle, 0.2 mm layers, four walls,
five top/bottom layers, 25-35% infill. Use a brim on the upright carriage and
small stops. Inspect overhangs and bridging in your slicer, especially the
rail's alignment pocket and the lock-nut loading slot. Remove any support or
brim residue from sliding surfaces and threaded pilots.

All rigid parts fit a nominal **220 x 220 mm bed**, before printer-specific
exclusion zones. The 214 mm rail leaves only 3 mm at either end on that bed:
omit a wide brim there or change the segment length for a smaller printer.
The design is intended to print without extensive supports in the supplied
orientations, but the actual slicer preview and first print decide this.

PLA is suitable for the first dimensional trial. Choose the installed material
after measuring enclosure temperature; PETG is a candidate for the final trial.
Use the same material/settings for rail coupons and final rail parts. No
temperature, friction, lifetime or load performance has been physically tested.

## Hardware

Lengths below are **under the head**, with no additional washers unless you
adjust length. Use small pan/wafer heads for PCB screws. The printed pilots are
intended for thread-forming screws for plastic; do not force unsuitable screws.

| Hardware | Quantity | Purpose |
| --- | --- | --- |
| M2 x 6 thread-forming screws | 4 | Camera through PCB into front posts |
| M2.5 x 8 thread-forming screws | 4 | Pi through PCB into 8 mm posts |
| M3 x 12 thread-forming screws | 4 | Front panel to shell, 9 mm engagement |
| M3 x 10 thread-forming screws | 4 | Through inside of housing rear into carriage, 7 mm engagement |
| M3 x 12 thread-forming screws | 4 | Two per recessed end stop, 4 mm rail engagement |
| M4 x 16 hex-head machine screw | 1 | Thumb lock; nominal 7 mm across-flats head |
| M4 hex nut | 1 | Captive carriage nut, nominal 7 mm across flats x 3.2 mm thick |
| Rubber/TPU disc, 8 mm diameter x 1.5 mm | 1 | Brake contact pad; may use BrakePad STL |
| Wall screws and suitable anchors | 8 | Rail holes are 4.5 mm; heads must be <=8 mm diameter |
| Small zip ties | 2 | Internal power cable restraint |
| 5 mm LED + wiring, 12 mm panel button | 1 each | Placeholder control sizes; choose exact parts before full panel print |

The status LED's flange fits within the 7 mm baffle bore. Secure it with a small
amount of suitable removable adhesive after checking fit. The button must accept
a 3 mm panel and its internal body/nut must fit within the 18 mm diameter,
25 mm depth allowance. Hardware shown in the viewer is simplified, not supplied.

## Assembly sequence

1. Print and test the fit coupons. Deburr openings and sliding edges. Verify
   that each pilot accepts its intended screw without splitting the post.
2. Insert the M4 nut into the carriage's slot from the **lower end**. Insert
   the M4 x 16 bolt into the knob's front hex recess and engage it in the nut.
   Place the brake pad in its 8.5 mm pocket facing the rail. A tiny dab of flexible
   adhesive on the screw tip can retain the pad; keep adhesive off the rail.
   Back the knob out enough that the pad does not drag during insertion.
3. Fasten the carriage to the enclosure with four M3 x 10 screws from inside.
   Its long section projects below the housing; the knob must remain reachable.
4. Feed the unplugged USB-C lead through the bottom opening. This assumes the
   plug overmold is no more than about 13 mm across. Fit a protective sleeve if
   needed, plug into the Pi, and secure the cable to the internal saddles with
   zip ties. The printed opening itself is a passage, not a sealing gland.
5. Screw the Pi onto its four posts. Confirm that no screw tip or mounting part
   touches a PCB component. The model allows 26 mm above the board for cooler,
   components and header plugs; check your actual assembly against that space.
6. Install the camera on the front-panel posts, lens facing outward. Add the
   LED and button, connect their wiring, and route the camera ribbon with a
   relaxed service bend. No repeated ribbon flex is needed during rail motion.
   Support the lid when opening it: it carries the camera and is tethered by the
   ribbon. Use the four M3 x 12 screws to close the panel.
7. Align the two rail sections vertically on a flat mounting surface with the
   tongue engaged. **Use the carriage straddling the joint as an alignment gauge**
   before final tightening. Fix both sections with the eight wall screws. Keep
   the screw heads outside the carriage path; the model allows <=8 mm heads.
8. Fit the lower stop. Slide the carriage onto the rail from the open upper end,
   then screw on the upper stop. The separate stops make insertion and servicing
   possible. Do not leave the sliding assembly without both stops fitted.
9. Support the enclosure with one hand, loosen the knob, choose a height, and
   tighten gently. The pad presses the rail and takes up play against the
   dovetail. Check holding force at several heights and across the joint before
   leaving it mounted. The stops limit travel but are not a rated safety catch.

**There is no validated holding-load rating yet.** Weigh the complete unit and
bench-test the actual printed rail, brake, fasteners and wall attachment with
that load, including repeated adjustment and button presses. After fit-up,
verify that sustained camera operation does not cause Pi throttling or excess
heat inside the enclosure. Keep the external power service loop free through
the full 300 mm of travel.

## Remaining measurements

The Pi model is confirmed. These dimensions remain provisional:

- Actual sensor board/lens: 25 x 23.862 mm PCB reference, 21 x 12.5 mm mounting
  centers; lens center assumed 9.5 mm from the top PCB edge, lens envelope
  15 mm diameter x 11 mm projection. PiCam-compatible holes do **not** guarantee
  identical lens position or component heights. The camera coupon checks this.
- Exact button and LED parts, printer model, material and useful bed area.
- Installed cooler, GPIO connectors, ribbon length/bend and USB-C plug overmold.
- Required wall location, useful height range and mounting surface.

The flash is **reserved space only**, not a working flash mount, diffuser or
electrical design. A future flash can use a revised front panel; it needs its
own chosen light source, optics, driver and heat management. The status LED
opening does not assume it can light a photograph.

## Geometry checks and rebuilding

```bash
bash hardware/enclosure-v2/build.sh
```

The build uses the installed FreeCAD 1.1.3 modeling engine without opening a
window. It checks all printable solids, closed meshes, pairwise printable-part
interference, hardware-to-shell/front clearances, sampled movement across the
rail joint and through both travel limits, and geometric retention by the
dovetail and end stops. Saved FCStd/STEP/STL files are reopened to check that
geometry, volumes, print dimensions and bed placement survive export.

These checks establish coherent printable geometry, **not physical fit or
structural certification**. The numeric report is in `output/validation.json`.
The generator asserts the minimum fixed-layout envelope when dimensions change;
larger layout changes may require edits to the Python recipe as well as JSON.
Rebuild with the model closed, or save manual FreeCAD edits to another filename.

## Dimension sources

- [Official Raspberry Pi 4 B mechanical drawing](https://datasheets.raspberrypi.com/rpi4/raspberry-pi-4-mechanical-drawing.pdf):
  85 x 56 mm board, 58 x 49 mm hole pattern, 3.5 mm edge offsets. Saved in `reference/`.
- [Official Camera Module 2 mechanical drawing](https://datasheets.raspberrypi.com/camera/camera-module-2-mechanical-drawing.pdf):
  25 x 23.862 mm board, four 2.2 mm holes, 21 x 12.5 mm pattern after orienting
  the ribbon downward. Saved in `reference/`. This establishes a reference
  pattern, not the dimensions of the user's different sensor/lens.
- [Raspberry Pi camera hardware documentation](https://www.raspberrypi.com/documentation/accessories/camera.html):
  shared board/hole geometry does not imply interchangeable lens clearance.

![Camera posts and locking carriage](output/mount-details.png)
