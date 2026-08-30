# Daily Mirror hardware prototyping plan

## Goal

Turn the working Raspberry Pi, IMX519 camera, pushbutton, and loose LED wiring
into a dependable enclosed appliance. The finished front face should expose
only two intentional features:

1. the camera lens through a fitted bezel; and
2. one illuminated, multicolor pushbutton or a button surrounded by an RGB
   ring.

Power should enter through the rear or bottom. The Pi, wiring, connectors,
resistors, level shifter, fasteners, and service access should be hidden inside.
This plan deliberately keeps the Raspberry Pi and proven software through the
first enclosed versions. A smaller compute platform is a later product decision,
not a prerequisite for eliminating jumper wires.

## The shortest path away from jumper wires

Do not jump directly from a breadboard to a custom PCB. Build one soldered
carrier on perfboard first. That version establishes the real wire lengths,
connector locations, mounting-hole pattern, thermal behavior, and enclosure
dimensions. Those facts become the input to the PCB rather than guesses made
in CAD.

Use this progression:

```text
breadboard proof
  → soldered perfboard + locking connectors
  → enclosed engineering prototype
  → custom Pi carrier PCB
  → refined enclosure + small pilot batch
```

## Target physical architecture

### Front assembly

- A rigid camera bracket locates the IMX519 module independently of the Pi.
- The lens sits in an open, matte-black bezel. Do not put ordinary acrylic in
  the optical path. Add an anti-reflection optical window only if impact or dust
  protection proves necessary.
- A panel-mount momentary button provides the only user control.
- An addressable RGB ring around that button provides every visible status.
  This replaces the separate green, yellow, and red LEDs.
- A short internal light baffle prevents the RGB ring from reflecting into the
  lens.

### Internal assembly

- The Raspberry Pi mounts on threaded standoffs to a removable internal plate.
- A small carrier board handles the button connector, RGB data-level shifting,
  power distribution, and test points.
- The camera ribbon is folded only within its specified bend radius and is
  retained so enclosure service cannot pull on the camera connector.
- Internal cable connections use keyed or locking connectors. Dupont jumper
  sockets are not acceptable after the perfboard phase.
- Power enters through a rear/bottom panel opening with strain relief. No bare
  power conductors or inline breadboard adapters remain inside.
- Vent slots are hidden on the bottom and upper rear to create a passive airflow
  path without exposing the electronics from the front.

## Electrical baseline

The current software uses BCM GPIO 2 for the active-low button and GPIO 17, 27,
and 22 for three active-high LEDs. During the perfboard phase, keep that working
interface unchanged so mechanical work can be tested without a simultaneous
software rewrite.

The RGB-button conversion is a separate, measured step:

- Button contacts: GPIO input to ground, using the Pi's internal pull-up.
- RGB ring: 5 V, ground, and one addressable data signal.
- Data signal: one 3.3 V-to-5 V logic-level shifter channel designed for a fast
  digital signal; do not rely on a direct 3.3 V connection in the enclosed unit.
- Add local bulk and ceramic decoupling at the ring connector.
- Add an inline data resistor near the level shifter output.
- Use a shared ground and size the 5 V wiring for Pi peak load plus the RGB ring
  at its configured brightness limit.
- Keep labeled test pads for 5 V, 3.3 V, ground, button, and RGB data.

Before ordering a PCB, update the device software to represent status as one
logical RGB output and verify every capture state on the actual ring. Keep the
three-LED implementation available behind a configuration choice until the new
hardware has completed a soak test.

## Phase 0 — freeze and document the working breadboard

**Purpose:** preserve a known-good reference before changing the mechanics.

Tasks:

- Photograph both sides of the working assembly and every Pi header connection.
- Draw a wiring schematic with GPIO names, resistor values, LED polarity, button
  contacts, power rails, and connector orientation.
- Record the exact Pi model, IMX519 board revision, camera ribbon length, power
  supply rating, button dimensions, and current LED part numbers.
- Measure the maximum current at idle, during autofocus/capture, during upload,
  and with all status LEDs active.
- Save one known-good capture and record focus, orientation, temperature, and
  software version.
- Label the current wiring physically before removing anything.

Exit criteria:

- Another person could reconstruct the breadboard from the schematic and parts
  list.
- A capture, offline queue, retry, reboot, and error indication all pass.

## Phase 1 — soldered bench prototype

**Purpose:** remove intermittent jumper-wire connections without yet solving
the final enclosure.

Build a small perfboard carrier with:

- a keyed cable or terminal block to the button/LED assembly;
- a short keyed harness to the Pi GPIO header;
- soldered current-limiting resistors and power distribution;
- strain relief for every wire leaving the board;
- mounting holes or adhesive standoffs; and
- labeled test points.

Use crimped connectors where parts must be removable and soldered joints with
heat-shrink where they do not. Use consistent wire colors and create a simple
harness drawing with pin numbers. Do not solder wires directly to the Pi or the
camera module.

Exit criteria:

- No breadboard or loose Dupont jumpers remain.
- The assembly survives being picked up, rotated, and lightly shaken without a
  reboot, false button press, or LED flicker.
- Fifty consecutive button captures succeed.
- Unplugging and reconnecting every serviceable cable cannot reverse polarity
  or shift a connector by one pin.

## Phase 2 — enclosure geometry prototype

**Purpose:** determine layout, camera alignment, usability, thermals, and service
access with inexpensive, quickly revised parts.

Create a rough 3D-printed or laser-cut enclosure around the perfboard assembly.
Start with a two-part shell and a removable internal mounting plate. Avoid
designing decorative curves until the component locations work.

Mechanical experiments:

- Test camera height and angle in the intended wall location before fixing the
  lens opening.
- Print at least three lens-bezel depths to find the shortest baffle that blocks
  LED flare without reducing the field of view.
- Test whether the button is comfortable and obvious at the installed height.
- Verify that the enclosure can be opened without disturbing camera alignment.
- Provide access to the SD card only after opening the enclosure.
- Put power and any temporary service port on the rear/bottom, never the front.
- Measure CPU temperature after one hour idle and after repeated captures with
  the enclosure installed.
- Check Wi-Fi signal at the final wall position with the enclosure closed.

Exit criteria:

- The front shows only the lens and illuminated button/ring.
- The captured image has no enclosure obstruction, internal reflection, or RGB
  flare.
- The unit can run continuously for 24 hours without thermal throttling.
- The enclosure can be opened, serviced, and reassembled without tools touching
  the lens or changing framing.

## Phase 3 — enclosed engineering prototype

**Purpose:** make one unit that behaves like the intended appliance even though
it still uses perfboard internally.

Improvements over Phase 2:

- Use threaded inserts or captured nuts instead of screws driven repeatedly
  into printed plastic.
- Add a real wall plate, keyholes, or French-cleat-style mounting feature.
- Add power strain relief and a hidden hardware power-disconnect method.
- Use a matte, opaque interior around the camera.
- Secure the camera ribbon and every harness at both ends.
- Replace the three discrete LEDs with the selected RGB ring and enable the
  single-RGB software mode.
- Add an internal label with device ID, power requirement, GPIO/connector
  revision, and recovery URL.

Validation matrix:

| Test | Pass condition |
| --- | --- |
| Cold boot | Ready indication and admin health recover without intervention. |
| Network loss | Captures queue locally; red/error state is understandable. |
| Network return | Queue drains and gallery/Turso metadata appear exactly once. |
| Power interruption | No queue corruption; unit recovers on restored power. |
| Button abuse | Holds, double taps, and rapid presses produce no duplicate capture. |
| Thermal soak | 72 hours enclosed with periodic captures and no throttling. |
| Optical test | Day/night captures show no bezel clipping or LED reflections. |
| Mounting test | Removing and reinstalling the unit preserves framing. |

Exit criteria:

- The unit operates for one week in its real location without opening the case.
- All failures are diagnosable through LED behavior and the LAN admin page.
- A second person can install and power it using a one-page guide.

## Phase 4 — custom Pi carrier PCB

**Purpose:** replace hand-wired perfboard with a repeatable assembly after the
mechanical and electrical interfaces are proven.

The first PCB should remain intentionally boring. It is a carrier/interface
board, not a redesigned computer. Include:

- a keyed connection to the Pi GPIO header;
- a locking connector for the button and RGB ring;
- the 3.3 V-to-5 V data-level shifter;
- decoupling, the RGB data resistor, and conservative power traces;
- clearly labeled polarity and connector pin 1;
- test pads for all important rails and signals;
- mounting holes matching the proven internal plate; and
- board name, revision, date, and open-source schematic reference.

Before ordering:

- Have someone other than the designer review the schematic and connector
  pinout against the Phase 0 wiring record.
- Print the PCB at 1:1 scale and physically place the connectors, fasteners, and
  enclosure over it.
- Run electrical-rule and design-rule checks.
- Order a small quantity and assemble one board before committing the rest.

Exit criteria:

- Two independently assembled boards pass the Phase 3 validation matrix.
- No bodge wires or cut traces are required.
- Board replacement takes minutes and does not require soldering inside the
  enclosure.

## Phase 5 — pilot enclosure and small batch

**Purpose:** learn whether the design is repeatable, installable, and maintainable
beyond the original unit.

- Build three to five complete units using the same PCB, harness lengths,
  fasteners, image, and installation instructions.
- Create a manufacturing BOM with exact manufacturer part numbers and approved
  substitutions.
- Add a connector/harness drawing, assembly order, torque guidance where needed,
  and a final inspection checklist.
- Give each unit a unique device ID and record its Pi, camera, PCB, and enclosure
  revisions.
- Track assembly time and every step that requires rework or tribal knowledge.
- Install units in different lighting and Wi-Fi conditions for at least two
  weeks.

Exit criteria:

- All pilot units pass the same acceptance test without hand tuning.
- Assembly and service instructions work for someone who did not design it.
- Known thermal, optical, electrical, and mounting margins are documented.
- The remaining reasons to leave the Pi platform are cost, size, or supply—not
  unresolved prototype reliability.

## Parts and tools needed for the next phase

The immediate purchase/build list is deliberately for Phase 1 and Phase 2, not
for a production run:

- small plated perfboard;
- appropriately sized stranded hookup wire in consistent colors;
- keyed locking connector housings, crimp contacts, and matching headers;
- crimp tool matched to the selected contact family;
- heat-shrink tubing and cable lacing or small tie mounts;
- Pi-compatible standoffs and assorted M2/M2.5/M3 fasteners;
- panel-mount momentary button or the selected illuminated RGB button;
- addressable RGB ring if it is separate from the button;
- 3.3 V-to-5 V logic-level shifter and supporting passives;
- multimeter and, ideally, a basic logic analyzer;
- calipers for component and enclosure measurements;
- opaque black material or prints for lens-bezel experiments; and
- two or three inexpensive enclosure iterations rather than one polished print.

Do not order a custom PCB until the Phase 3 unit has completed its one-week
installed soak. Do not choose an ESP32 solely to make the enclosure smaller;
that changes the camera, software, storage, diagnostics, and update system at
the same time. First prove the physical product around the working Pi.

## Immediate next sprint

The next concrete milestone is **Phase 1: soldered bench prototype**.

1. Draw and review the current schematic.
2. Decide whether the first perfboard keeps the three LEDs or simultaneously
   introduces the RGB ring. Keeping the three LEDs is lower risk; introducing
   the ring reduces one later rebuild.
3. Choose one locking connector family and document its pinout.
4. Build the perfboard and harness beside the untouched working breadboard.
5. Move one circuit at a time, testing button and each LED after every move.
6. Run the 50-capture mechanical test.
7. Measure the resulting assembly and begin the enclosure layout only after it
   passes.

The recommended choice is to keep the known three-LED circuit for the first
soldered board, prove the harness and mechanics, then convert that same board to
the RGB ring before freezing the enclosure front panel. This separates wiring
reliability from the LED software change while still reaching the intended
two-feature front face during Phase 2.
