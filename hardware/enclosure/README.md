# Daily Mirror enclosure concept 01

Generated with installed FreeCAD 1.1.3 via its Python API and FreeCADCmd.
No MCP or extra Python CAD package is needed. Run from the repository:

```bash
./hardware/enclosure/build.sh
FreeCADCmd hardware/enclosure/verify_exports.py
python hardware/enclosure/render_preview.py
```

The generator also works through FreeCAD's Python console:

```python
import runpy
runpy.run_path('/home/drew/work/daily_mirror/hardware/enclosure/build_enclosure.py')
```

The command-line generator and saved-file round trip were tested. An attempted
GUI screenshot using Qt's offscreen platform could not create an OpenGL context;
GUI macro execution was not verified in that environment. This does not affect
headless modeling. `OpenEnclosure.FCMacro` is supplied for the normal desktop GUI:
run it with Macro > Macros to open the document, hide construction history, color
the parts, and make reference envelopes transparent. It contains this checkout's
absolute path; adjust if moving the repository. Avoid rebuilding with the same
model open and unsaved; save manual edits separately first.

## Files and editability

- `build_enclosure.py`: authoritative Python modeling recipe, dimensions in mm.
- `output/DailyMirrorEnclosure.FCStd`: native editable primitive/boolean feature
  tree, including a separate future slider study and hardware reference volumes.
  This is not a fully constrained sketch-based Part Design model. Individual
  primitive properties can be edited; coordinated layout changes belong in the
  script. Not every placement is driven from W/H/D.
- `output/{Base,Lid,CameraAdapter}.{step,stl}`: solid exchange and printable mesh
  exports for review. They are not yet qualified for a production print.
- `output/enclosure-preview.png`: preview rendered from actual CAD meshes.
- `output/validation.json`: geometry and closed-mesh validation results.

## Design

The base is 180 wide × 180 tall × 70 deep, with 3 mm walls/back. The lid adds
3 mm; the replaceable camera plate adds another 3 mm locally. The clear shell
interior before bosses is 174 × 174 × 67 mm. The model reserves a separate
48 × 65 × 45 mm wiring/perfboard area next to the Pi. These are deliberately
roomy initial dimensions, not the minimum case size.

The Pi envelope is 85 × 56 mm, with four standoffs on a 58 × 49 mm pattern.
Reference: [official Pi 4 mechanical drawings](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#schematics-and-mechanical-drawings).
A 30 mm component/cooler envelope above the PCB is a planning allowance, not an
exact connector model. Check cooler height, GPIO plug, ribbon and power bends.
The open lower service mouth allows temporary cable routing; exact Pi port
cutouts, a removable cable cover and strain relief are follow-up work.

The lid has a removable camera plate rather than a permanently fixed camera
hole. Its 18 mm lens opening, 25 × 24 mm PCB reference, and 21 × 12.5 mm screw
pattern are provisional. Confirm the actual camera before printing this plate.
The reference board position needs 7.4 mm spacing between the PCB's front surface
and the plate underside. Use suitable spacers and screws once the camera/lens
protrusion is known; a ribbon restraint and optical baffle remain to design.
LED and button holes are deliberately undrilled until their parts are known.
The LED wiring and single-indicator software mode remain separate pending work.

## Height adjustment decision

Assumption pending confirmation: “foot slider” means approximately 300 mm of
vertical travel, not a foot-operated mechanism.

1. **Fixed box first:** flat rear adhesive lands for a trial wall position.
2. **Slide the whole enclosure:** camera, Pi and internal wiring travel together;
   only the external power cable flexes. This is the recommended first moving
   prototype because it preserves the camera connection. Allow a restrained
   power service loop at every height. It moves more weight and moves the button.
3. **Slide a camera-only pod:** lighter and keeps the control button stationary,
   but requires a separate pod and a validated flexible connection. Do not treat
   the existing CSI ribbon as a repeated-flex cable; validate cable type, bend
   radius, length and image reliability. A compatible USB camera or a camera
   extension system could simplify that connection, with compatibility tradeoffs.
4. **Fixed height with tilt/wider framing:** simplest mechanics; test before
   committing to a rail. Tilt changes viewpoint and perspective, and cropping
   cannot reproduce eye-level capture from a different height.

The included study uses a 420 mm C-channel with a 100 mm carriage and 10 mm end
stops, leaving 300 mm travel. The carriage and enclosure share a 40 × 40 mm M4
clearance-hole pattern. The study is NOT an assembly-ready slider: end stops
currently prevent insertion, the top stop must become removable, a positive
lock/detent and fastener access must be designed, and clearance/load testing is
required. Slider geometry is excluded from printable exports. A single 420 mm
rail may exceed the printer bed; prefer an available metal rail or segmented
validated design for the next iteration. Confirm useful travel with actual
short/tall users at the intended camera distance.

## Wall mounting and print gate

The two 30 × 130 mm rear reference zones are reserved flat adhesive lands, not
claims about a particular Command product. Choose the strips for the measured
assembled mass and approved surfaces, leave removal tabs accessible below, and
follow the [manufacturer's instructions](https://www.command.com/3M/en_US/command/how-to-use/).
A hanging-weight rating is not a validation of repeated sliding/button forces.
For an adjustable rail, prefer screw mounting to suitable wall structure; an
adhesive-only moving design needs separate validation. The rear four-hole
interface also provides a path to a wall/rail adapter.

Before a full print: confirm printer bed/material, camera and controls, pilot
hole fit, screw lengths, ribbon routing, camera clearance and wall orientation.
Print a standoff/camera-fit coupon first. Print the shell back-down and lid
face-down after inspecting supports in the slicer; vents and corner bosses
have overhangs that may need support or a later chamfered redesign. Use proper
spacers so screws cannot stress the camera PCB. Then bench-test optics, enclosed
thermals and mounting load before leaving the unit unattended on the wall.
