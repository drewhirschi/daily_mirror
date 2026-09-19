# Daily Mirror custom PCB - A1 component study

Open **daily-mirror-p4.kicad_pro** in KiCad 10. This is an editable starting
project with the connected controls schematic and **64 additional candidate
components** on four new schematic pages and the board. It now has 80 electrical
references plus four mounting holes. The imported main components have no circuit
connections yet; the board has no routed traces. **Do not order this board.**

See [sourcing.md](sourcing.md) for the part choices, 3D-model limitations and
open-reference workflow. [candidate-parts.csv](candidate-parts.csv) records
the new parts and their sources; it is not an order-ready BOM.

An editable, routed Olimex P4 development-board reference is in
[reference/olimex-esp32-p4-devkit](reference/olimex-esp32-p4-devkit).
Open its `ESP32-P4-DevKit_Rev_D1.kicad_pro` separately to inspect the original
connected design. It has not been merged into the Daily Mirror circuit.
The previous A0 files/previews are saved in `revisions/a0`.

## Decisions from this session

- Keep the existing Arducam IMX519 as the camera baseline; preserve the goal of
  16 MP stills. Use a purchased camera module on a ribbon cable. The image
  sensor and lens do not need to be integrated onto the main custom PCB.
- USB-C wall power; a new enclosure designed around the final electronics.
- Cost matters: favor a standard four-layer main board for the P4, common
  stocked components, and assembly on one side where practical. Evaluate total
  assembled cost, including setup and shipping, rather than promotional bare-PCB
  prices. No per-unit budget or production quantity has been specified yet.
- One small four-pin RGB status indicator, plus a momentary capture/pairing button.
- **White-only emitters are preferred** around the perimeter for photographs.
  The three-color CL651-3S modules are optional prototype parts, not a required
  final component. Choose documented white LEDs/modules and their current
  drivers once the brightness, optical layout and power budget are established.
- Snap-in removable diffuser above the light modules. Include an opaque lens
  baffle, room between diffuser and emitters, and space behind the modules for
  a heat-spreading mount. Diffuser material, thickness, gap and clips need an
  optical/mechanical prototype; the current enclosure models are Pi-specific.

## Important processor finding

**The ESP32-P4 is still a candidate, not a confirmed solution for full-resolution
IMX519 capture.** Having a matching MIPI interface does not establish camera support.

Espressif's current ISP header defines `ISP_LL_HSIZE_MAX` as **1920**. Its ISP
driver rejects larger `h_res` values. The IMX519 full image is **4656 x 3496**.
Thus the ordinary P4 ISP processing path cannot process a full-width 16 MP frame.
This is an image-processing-path limit, not proof that every raw CSI/DMA capture
path is impossible. A bypass-and-upload approach would need a sensor driver,
working DMA/buffer handling, exposure/focus control and a server image pipeline.
It has not been implemented or tested here.

The official sensor list does not list a native IMX519 driver. Its Arducam
PIVARIETY entry does not establish support for the user's native IMX519 module.
The community IMX driver project checked lists other IMX sensors, not IMX519;
its results cannot be assumed to apply to this camera.

A packed RAW10 full frame is 20,346,720 bytes (19.40 MiB); a 16-bit-per-pixel
buffer takes 32,554,752 bytes (31.05 MiB), before firmware and other buffers.
Capturing infrequently reduces average throughput, but not those per-frame
buffer requirements. Streaming approaches must be proven separately.

**Next hardware gate:** demonstrate a full-field still from the actual IMX519
on the proposed processor, with usable exposure/focus and successful upload.
If P4 raw capture cannot meet that requirement, select a processor with a
validated IMX519 pipeline before drawing its main circuit. A custom board
does not require a Raspberry Pi, and a P4 has not been chosen merely for size.

Source snapshot: ESP-IDF commit `c712a0dde385d659a1470a136251980d31a70bc1`,
checked September 14, 2026. See [reference/isp-limits.md](reference/isp-limits.md).

## What is actually drawn

| Reference | Circuit | Current choice |
| --- | --- | --- |
| D1 | Common-anode RGB indicator | Project symbol preserves documented pin order: 1 blue, 2 green, 3 common positive, 4 red |
| R1-R3 | One current-limiting resistor per color | 1 kohm provisional; adjust after measuring forward voltage and brightness |
| R5-R7 | Pull each color control high while GPIO is floating | 100 kohm; high = off, low = on |
| SW1 | Normally-open momentary button | Generic 6 mm tactile footprint, 4.3 mm actuator height candidate |
| R4 / C1 | Button pull-up and filter | 10 kohm / 100 nF; approximately 1 ms RC; use firmware debounce as well |
| TP1-TP6 | Future processor/control test access | 3.3 V, ground, blue, green, red, button |
| H1-H4 | Board-only mounting holes | M3 clearance, provisional 92 x 92 mm spacing |

The LED's common positive leg has **no fourth series resistor**. Three separate
color resistors avoid the brightness interactions caused by sharing one resistor.
The 1 kohm starting values cap direct GPIO current below 3.3 mA per channel at
3.3 V; final brightness, GPIO limits and resistor power must be checked against
the chosen parts. This is only the small indicator circuit, not the COB driver.

The RGB symbol follows [the existing wiring record](../../device/README.md),
but the loose LED's view/orientation, lead size and lead order must be confirmed.
The candidate **wide-pin** footprint uses formed leads at 2.54 mm pitch; it is
not a claim that the unbent loose LED has that pitch. The generic button's
mechanical height must also be chosen with the new faceplate.

The board is a **100 x 100 mm, four-copper-layer space study**. None of those
dimensions, mounting holes or layer rules is a final manufacturing specification.
The PCB now contains the imported candidate footprints, including eight white
emitters. The camera faceplate clearance is still a drawing guide; the separate
camera module, cable, diffuser and enclosure have not been mechanically modeled.
The 3D preview represents these placed components; five package types use explicitly
approximate models. See the sourcing notes before using this for mechanical fit.

## Existing CL651-3S prototype light modules

The final board can use white-only parts instead. The notes below apply only
if the already-owned three-color modules are used for prototype measurements.

The supplied photo and the manual linked from the exact
[Temu listing](https://www.temu.com/goods.html?goods_id=606385509136562&sku_id=99388093393455)
identify a three-color, common-negative COB module. The manual explicitly maps:

| Pad marking | Seller's stated function |
| --- | --- |
| `B+` | White positive |
| `W+` | Blue positive |
| `R+` | Red positive |
| `-` | Shared negative |

This unusual B/W assignment should be checked at low current on a current-limited
bench supply before wiring a batch. Only white and negative will be used;
unused red/blue positives stay unconnected.

The listing/manual states **3.2-3.7 V** and warns of heating at 3.7 V. It provides
**no rated current, pulse current, pulse width, thermal resistance, CRI or CCT**.
It is not a complete electrical datasheet. Do not substitute its broad listing
category `<=36 V` for the actual operating voltage.

Plan a controlled-current path for each white module, with default-off enable
and a bounded illumination time. Do not drive COB modules from GPIO, connect
them directly to USB's 5 V, or infer a current limit from voltage alone. Do not
parallel bare modules behind one driver without a validated current-sharing scheme.
The appropriate buck/linear topology, current sense resistor, module count,
connector, copper area and USB supply rating remain unresolved.

These parts already have their own substrate. The likely integration is short
two-wire connections to modules mounted around the faceplate/perimeter, with
control electronics on the main PCB. Direct mechanical/electrical attachment
to the main board needs measured pad positions, mounting holes and a thermal
design. No guessed CL651-3S solder footprint is included.

### Flash timing and diffuser

Short duty cycles reduce average heat: `average electrical power = V * I *
on-time * flashes-per-second` for constant rectangular pulses. Peak junction
temperature and safe pulse current still matter; average power alone does not
establish safety. Repeated presses and a stuck-on software state must be tested.
No pulse overdrive is assumed without a manufacturer rating.

The IMX519 is a rolling-shutter camera. Start with steady white illumination
through the full frame exposure/readout window, then characterize the shortest
uniform pulse from actual captured images. Avoid unsynchronized PWM during
capture. Test for bands, face shadows, clipping, diffuser hot spots, lens flare
and enclosure temperature at the maximum intended repeat rate.

Record current, measured light output and duration before deciding whether a
small heat spreader is enough. Keep diffuser clips clear of the thermal path.

## What the main circuit still needs

- Camera-proven processor, package/revision and memory capacity.
- For a P4 design: external flash, reference crystal, core regulator, PHY and
  other supplies/decoupling, reset/boot circuitry and programming access. Use
  Espressif's revision-v3-or-later reference; older revisions differ electrically.
- Wi-Fi/BLE companion and antenna layout if using P4. ESP32-C6-MINI-1 with SDIO
  is the reference direction; P4 itself does not contain a Wi-Fi/BLE radio.
- USB-C connector, CC handling, input protection, regulator and current budget.
  Two CC pull-downs establish a sink attachment but do not by themselves authorize
  a 3 A load. Detect the source's available current or negotiate PD as required.
- Exact Arducam module SKU/revision and ribbon. Pi-style connectors can be 15-pin
  1 mm or 22-pin 0.5 mm. Confirm connector-side pin numbering, contacts, supply,
  controls, level shifting and cable orientation before assigning the footprint.
- CSI differential pairs routed against the chosen fabricator's stackup and
  Espressif's guidelines; impedance is not established by arbitrary trace widths.
- Flash/illumination driver and any local storage required for failed uploads.

The phone experience is firmware/app work too: BLE discovery while pairing,
authenticated ownership setup, transfer of Wi-Fi credentials, association and
account registration, then normal capture/upload. Add long-press recovery,
factory reset, signed updates and upload retry. KiCad does not implement this.
The existing Pi program invokes `rpicam-still`; that part cannot run unchanged
on the P4. Backend protocols and device states can be reused where applicable.

## KiCad setup and working together

KiCad **10.0.6** was installed without its library packages. The matching official
Arch packages (`kicad-library` and `kicad-library-3d`, 10.0.6-1) were downloaded,
their signatures verified against the installed Arch keyring, and extracted to:

`/home/drew/.local/share/kicad/10.0/official-10.0.6`

This per-user install avoids the system installer's password requirement. It
does not register those two library packages with pacman. KiCad's global library
table references and `KICAD10_*` paths were updated. The previous settings are in:

`/home/drew/.config/kicad/10.0/before-daily-mirror-20260914-214132`

The dangling default reference to a nonexistent optional design-block library
was also removed; its original table is in that backup. Design blocks are not
needed for this project.

Restart KiCad once if it was already open. Then:

1. **File > Open Project** and select `daily-mirror-p4.kicad_pro` in this directory.
2. Open the `.kicad_sch` for the architecture and control circuit.
3. Open the `.kicad_pcb`; enable `User.Drawings` to see the reserved areas.
4. Use **View > 3D Viewer** to inspect the placed candidate parts.

The current 3D render is `output/board-3d.png`. The STEP assembly in
`output/daily-mirror-a0.step` contains the current board and the placed component
models, and can be opened in mechanical CAD for enclosure work. Neither includes
the unselected processor, camera module, power connectors or white emitters.

### Reading the design

The schematic describes electrical connections. Green lines are wires in that
diagram; repeated net names identify the same electrical connection without
drawing a long line across the page. The PCB shows real component locations and
solder pads. Its connections become copper **traces** routed between pads;
this initial board has no routed traces yet. In the PCB editor, thin ratsnest
lines can show which pads still need connections; those are guides, not copper.
The 3D viewer shows physical models, not schematic symbols or ratsnest lines.

| Label | Meaning |
| --- | --- |
| J1, J2 | Connectors, such as camera ribbon or USB |
| U1, U2 | Integrated circuits/chips; exact function depends on the part |
| R1, R2 | Resistors |
| C1, C2 | Capacitors; often resemble tiny resistors |
| D1 | Diode or LED |
| SW1 | Switch/button |
| TP1 | Test point |

For a sourced part, check three separate CAD representations: its schematic
symbol (electrical pins), footprint (solder pads and holes), and STEP/3D model
(physical shape). Matching physical shape alone does not prove the pin mapping
or circuit works. Use exact manufacturer part numbers and package drawings.

### Making a physical unit

PCB fabrication makes the patterned copper board. PCB assembly adds and solders
the purchased components. An assembly order typically needs Gerbers/drill files,
a bill of materials with exact part numbers, and placement/orientation data.
Fabricators do not print silicon image sensors or lenses. A camera module can
be purchased separately and connected after board assembly.

JLCPCB offers an assembly parts library and sourcing/consignment options;
PCBWay offers turnkey, partial-turnkey and customer-supplied component assembly.
Stock, exact packages and service eligibility must be checked before choosing
the final bill of materials. Firmware flashing and functional testing require
their own agreed process; soldering the board does not establish that it works.

- [DigiKey CAD models](https://www.digikey.com/en/product-highlight/a/accelerated-designs/ultra-librarian)
- [KiCad 10 PCB and 3D documentation](https://docs.kicad.org/10.0/en/pcbnew/pcbnew.html)
- [JLCPCB bill of materials requirements](https://jlcpcb.com/help/article/bill-of-materials-for-pcb-assembly)
- [JLCPCB parts sourcing](https://jlcpcb.com/help/article/how-to-build-your-own-parts-library-in-jlcpcb)
- [PCBWay component sourcing](https://www.pcbway.com/pcb_prototype/Electronic_Components.html)

There is no callable KiCad MCP in this session. We can edit the native files
and use KiCad CLI for validation and previews. Close the affected editor before
external edits and reopen it afterward; do not assume live reload. For edits
made in the schematic editor, **Update PCB from Schematic (F8)** transfers changes.
The `.kicad_pro` file is the project, `.kicad_sch` is the circuit, and
`.kicad_pcb` is the physical board.

Small project-local symbol, footprint and 3D libraries make the study portable.
The generator refuses to overwrite an existing design. Interactive KiCad files
are the source of truth once editing begins. To reproduce the initial study in
a separate directory:

```sh
python build_starter.py --output /tmp/daily-mirror-pcb-study
```

## Checks

See [output/validation.json](output/validation.json) for the final counts.
The controls schematic still has zero ERC violations and its nine nets match
the board pads. All 80 schematic component pin-number sets match the footprints,
with J2's additional mechanical mounting tabs explicitly accounted for. All 74
placed model references resolve. No component courtyards overlap.

The whole project has **377 ERC findings** from unconnected/undriven candidate
pins and **96 DRC findings** (USB pad nets/connector hole clearance and silkscreen
size/overlaps). The 21 unrouted controls connections remain; because the new
circuits have no assigned nets, that number does not count their future routing.
No Daily Mirror Gerbers, assembly order or firmware release has been produced.

## Sources

- [Espressif P4 reference board](https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32p4/esp32-p4-function-ev-board/user_guide.html): P4/C6 architecture.
- [P4 schematic guidance](https://docs.espressif.com/projects/esp-hardware-design-guidelines/en/latest/esp32p4/schematic-checklist-esp32p4.html): revision changes and support circuitry.
- [P4 layout guidance](https://docs.espressif.com/projects/esp-hardware-design-guidelines/en/latest/esp32p4/pcb-layout-design-esp32p4.html): signal and power layout.
- [Espressif sensor support](https://github.com/espressif/esp-video-components/tree/master/esp_cam_sensor): supported sensors.
- [Arducam IMX519 documentation](https://docs.arducam.com/Raspberry-Pi-Camera/Native-camera/16MP-IMX519/): resolution, RAW formats and rolling shutter.
- [Community IMX driver project](https://github.com/mushBrainDave/esp32-p4-imx-camera): context only; not an IMX519 compatibility claim.
- [Raspberry Pi camera documentation](https://www.raspberrypi.com/documentation/accessories/camera.html): connector families.
- [Seller's LED manual](reference/cl651-3s-manual.pdf): voltage, terminal labels and heat warning; no current rating.
