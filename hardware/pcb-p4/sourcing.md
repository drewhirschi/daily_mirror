# Sourced parts and open P4 designs

## What was imported

The A1 project adds **64 candidate component instances**, with **19 symbol types**
and **21 footprint types**, to the original 16-reference controls circuit. All
80 electrical references appear in the schematic and PCB. Component pin numbers
were compared against footprint pad numbers; J2 has additional mechanical `MP`
tabs, which are not ribbon contacts. Current P4 pin 54 is `VDD_HP_1`.

Main circuits are not yet wired. No no-connect markers were added to conceal that
work. ERC reports the open pins; DRC still reports unset USB connector nets,
silkscreen issues and connector hole spacing against provisional board rules.
Physical component courtyards no longer overlap. The checks are import and
placement checks, not an electrical or manufacturing approval.

| Function | Imported candidate | Status |
| --- | --- | --- |
| Processor | Espressif ESP32-P4NRW32X | Current P4X symbol, 32 MB in-package PSRAM; IMX519 capture unproven |
| Wi-Fi and phone pairing | ESP32-C6-MINI-1-N4 | Manufacturer footprint and STEP; SDIO/firmware/antenna integration pending |
| Firmware flash | Winbond W25Q128JVSIQ | 16 MiB, SOIC-8 5.3 mm package |
| USB-C | GCT USB4105-GF-A | Footprint and KiCad STEP; final stake-length/order suffix pending |
| USB current detection | TI TUSB320IRWBR | 5 V sink direction; detects advertised current, does not negotiate USB PD |
| Main 3.3 V supply | TI TPS62132RGTR | 3 A buck candidate; complete output network and load budget pending |
| P4 core supply | TI TLV62569DBVR | Requires P4 rev3+ feedback/control circuit |
| Camera supply switch | TI TPS22919DCKR | Confirm module current, rails and startup behavior |
| USB ESD | ST USBLC6-2SC6 | Data-line protection; input/CC protection still needs design |
| Camera ribbon connector | Molex 2005280150 | 15 contacts, 1 mm pitch, bottom contact; verify actual ribbon/module |
| White emitters, D2–D9 | Cree XPGDWT-U1-0000-00J3E | 5000 K, minimum 90 CRI, quantity eight provisional |
| White LED driver | TI TPS61165DBVR | Constant-current boost candidate; tentative eight in series at 100 mA for optical tests |
| Inductors | Coilcraft XAL4020-222MEC / XAL4040-103MEC | Initial package/value choices; regulator-loop and current calculations pending |
| Boost diode | Vishay SS16-E3/61T | 60 V Schottky candidate; peak current/loss checks pending |
| P4 crystal | Abracon ABM8-40.000MHZ-10-1-U-T | 40 MHz / 10 pF, manufacturer test report; current ordering/temperature qualification pending |
| Decoupling and resistors | Murata GRM / Yageo RC families | Candidate MPNs and packages recorded; quantities and values remain preliminary |
| Boot and reset | Two 6 mm tactile switch positions | Generic package; B3F-1000 is a documented 4.3 mm-high candidate |

The new parts, MPNs, source links and individual qualifications are in
[candidate-parts.csv](candidate-parts.csv). Existing controls are listed in
[parts.json](parts.json) and README. A complete order list cannot yet be frozen:
camera processor compatibility, protection/inrush circuitry, final support-part
counts, flash/SDIO series terminations, antenna clearances, and light power/thermal
requirements remain open. Inventory and assembler stocking were not confirmed.
No parts were purchased.

The older TPS62133 is **5 V output**, not the intended 3.3 V part; A1 uses
TPS62132. The TPS61165's **1.2 A rating is its switch rating**, not a promise of
1.2 A output at the boosted LED voltage. Actual output capability requires a
converter power/loss calculation. The MIPI PHY uses the P4's reference power
scheme; it is not an arbitrary external 1.8 V rail.

## Camera connector requested for this board

**J2 is the camera ribbon connector**, placed to the right of the camera mounting
area. Its board label is `CAMERA CSI / 15P`. The intended interface is the standard
Raspberry Pi-style **15-contact, 1 mm pitch FFC** camera connection, carrying
MIPI CSI-2 signals. The physical connector candidate is Molex 2005280150.
The camera sensor and lens remain on the purchased module, connected by ribbon.

[Raspberry Pi's connector documentation](https://www.raspberrypi.com/documentation/accessories/camera.html#camera-connector-pinout-15-pin)
distinguishes this standard connector from the smaller 22-pin connector on Pi 5
and Pi Zero boards. The chosen footprint is the standard 15-pin format. Electrical
pin assignment and cable contact orientation still need validation when the camera
circuit is connected; the matching format alone does not establish camera support.

## What R1–R7, C1, D1 and TP1–TP6 mean

| Marking | Explanation |
| --- | --- |
| D1 | Four-legged RGB indicator: three colors plus a shared positive leg. Mixing colors gives approximate white; the separate white LEDs illuminate photographs. |
| R1, R2, R3 | One series current-limiting resistor for each blue, green and red element. No resistor is needed on the shared fourth leg. |
| R4 | Pulls the button input high while the button is released. |
| R5, R6, R7 | Pull the three color-control signals high so the common-anode LED stays off while processor outputs are floating. |
| C1 | Button filter capacitor; R4/C1 form about a 1 ms filter. Firmware debounce is also needed. |
| TP1, TP2 | Exposed copper test pads for 3.3 V and ground. |
| TP3, TP4, TP5 | Test pads for blue, green and red control signals. |
| TP6 | Test pad for the button input. |

The old image had only one capacitor because only the button/indicator circuit
had been populated. C2–C28 now illustrate the main circuit's preliminary support
population. They are not connected to the required rails yet. Test pads are part
of the manufactured copper pattern, not six extra components to buy.

For the original controls, Yageo RC0603FR-071KL (1 kΩ), RC0603FR-0710KL (10 kΩ)
and RC0603FR-07100KL (100 kΩ), and Murata GRM188R71C104KA01D (100 nF), are package
candidates. The loose RGB LED's manufacturer and physical lead orientation are
still unknown; the existing symbol preserves the recorded B/G/common+/R order.

## 3D and library provenance

- KiCad symbol/footprint/package assets come from official **10.0.6** libraries.
- P4X and C6 symbol/footprint assets come from
  [Espressif's official KiCad library](https://github.com/espressif/kicad-libraries)
  at commit `dd76561812ab300351234ba6e0ec1295641796f0`.
- The C6 STEP model is provided by Espressif. Stock package models are KiCad
  representations; they are not certifications of a selected manufacturer's part.
- Five types lack a matching available model: **P4, camera FFC, TUSB320, TPS62132,
  and XP-G3 LED**. Local approximate body/package envelopes make their placement
  visible. These are explicitly recorded in `libraries/import-manifest.json`.
  The XP-G LED land pattern is a candidate for the XP-G3 and still needs a
  manufacturer recommended-land-pattern audit. It is not an exact XP-G3 STEP.
- All referenced models live under `libraries/3dmodels`, with project-relative
  paths. STEP exports substitute the paired body-envelope STEP for approximate
  VRML models. Small lens/latch detail may differ between the preview and STEP.
- Neither the camera module nor the diffuser/enclosure is included in this board
  model. Nominal package models are not sufficient to approve enclosure fit.

License notices and hashes accompany the assets. The generator only creates a
new directory and retains the original A0 snapshot as its input, so it does not
overwrite interactive edits when rerun.

## Layer count and cost target

The user is cost-sensitive and is comfortable with additional layers when the
price difference is modest. The current board already has four copper layers.
For a P4 implementation, retain four as the baseline:
[Espressif's layout guidance](https://docs.espressif.com/projects/esp-hardware-design-guidelines/en/latest/esp32p4/pcb-layout-design-esp32p4.html)
specifies at least four layers for high-speed signal quality and RF coexistence.
Its suggested arrangement is top signals/components, solid ground, power, and
bottom signals. The manufacturer's dielectric thicknesses and copper weights
must be chosen before final impedance-controlled routing; they are not finalized.

Favor standard fabrication materials, ordinary through-vias and one-side assembly
where practical. Six or more layers need a routing/electrical justification and
an actual cost comparison. Do not remove the ground-reference layer to chase a
low bare-board headline price.

Qualifying small-prototype promotions can make four-layer bare boards inexpensive,
but they are not an assembled device quote. Parts, setup, soldering, inspection,
shipping and order quantity all affect delivered cost. P4's 0.35 mm pin spacing
also needs an assembler capability check; JLCPCB currently lists 0.4 mm for
Economic PCBA and 0.35 mm for Standard PCBA in its
[prototype assembly capabilities](https://jlcpcb.com/solutions/pcb-prototype-assembly).
No assembler tier or device price has been committed.

A compact four-layer main board plus a simpler separate light board is a future
cost comparison against the current 100 x 100 mm combined study. Extra connectors,
assembly and mechanical work may offset the area savings. Do not split the design
solely on an assumed saving; quote both if the enclosure/optics make it useful.

## Reusing an open P4 circuit

### Native KiCad reference downloaded

[Olimex ESP32-P4-DevKit](https://github.com/OLIMEX/ESP32-P4-DevKit) provides real
KiCad schematic and PCB sources, including routed copper and local 3D assets.
Revision **D1**, commit `26705d36407a07324348927dfd30fbf4ffc1d94c`, is copied into
`reference/olimex-esp32-p4-devkit`. Its original files are retained unchanged;
`SOURCE.json` records file hashes. The original schematic parses in KiCad 10 and
its PCB renders with the supplied models. External Olimex libraries would need
resolution before using Update PCB from Schematic in that reference project;
the files contain their current symbols/footprints for inspection.

This reference has P4, power, flash, USB, camera/display connections and other
development-board functions. It does not supply our C6 radio/phone provisioning
or establish IMX519 support. It uses a combined `NC(VDD_HP_1)` pin-54 symbol and
a different regulator implementation. Its pin 54 connects to R35/C30. That is
evidence to audit against the current P4X reference, not a reason to copy the
old circuit unchecked.

Olimex's README states **CERN Open Hardware Licence v2, Strongly Reciprocal** for
hardware. Its root `LICENSE` is GPL for software; do not mistake that file for the
hardware license. Retain attribution and follow the hardware license's source
requirements when distributing a derivative. These reference circuits have not
been copied into the Daily Mirror main schematic in A1.

### Espressif reference

Espressif publishes
[P4X Function EV Board and P4X EYE reference designs](https://docs.espressif.com/projects/esp-hardware-design-guidelines/en/latest/esp32p4/related-documentation-and-resources.html).
Those archives use **OrCAD Capture `.DSN` and Allegro `.brd`**, so they are not a
native KiCad schematic import. Their terms are linked on that page. Use the
current revision-v3-or-later reference and the
[schematic checklist](https://docs.espressif.com/projects/esp-hardware-design-guidelines/en/latest/esp32p4/schematic-checklist-esp32p4.html)
to check or redraw a core circuit in KiCad.

### How the reuse works

1. Select the exact processor revision and suitable reference/license.
2. Copy useful **connected schematic sections** into a child sheet: power,
   crystal, flash, reset/boot and programming, including their support components.
3. Map the camera, radio, status LED, button and light-driver signals to available
   pins. Audit voltage levels, boot straps and supply sequencing.
4. Bring the resulting footprints into the PCB. Where useful, copy matching
   placement/routing from the source board and remap references/nets carefully.
5. Adapt the outline and component locations; route connections against a chosen
   fabricator's layer stack. Moving a component can invalidate the copied routing.
6. Check the final schematic/PCB, then prototype camera capture and illumination.

Copying a reference circuit puts the chips directly on our custom PCB. A different
option is soldering a complete purchased processor module onto the PCB; that can
reduce layout work, but it changes cost, dimensions and the available interfaces.

## Primary component references

- [Cree XP-G3 data sheet](https://downloads.cree-led.com/files/ds/x/XLamp-XPG3.pdf),
  current white ordering table: `U1` 90-CRI code, `3E` 5000 K kit.
- [Molex 2005280150](https://www.molex.com/en-us/products/part-detail/2005280150)
- [TI TPS62132](https://www.ti.com/product/TPS62132),
  [TPS61165](https://www.ti.com/product/TPS61165),
  [TUSB320IRWBR](https://www.ti.com/product/TUSB320/part-details/TUSB320IRWBR)
- [Omron B3F switch data sheet](https://components.omron.com/us-en/datasheet_pdf/A070-E1.pdf)
- [Murata 100 nF data sheet](https://search.murata.co.jp/Ceramy/image/img/A01X/G101/ENG/GRM188R71C104KA01-01.pdf)
- [Yageo 10 kΩ data sheet](https://www.yageogroup.com/component-documentation/download/specsheet/RC0603FR-0710KL)
