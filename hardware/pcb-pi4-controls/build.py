#!/usr/bin/python3
"""Generate the Daily Mirror Pi 4 case control board (draft) from scratch.

Usage (system Python, which has KiCad 10's pcbnew module):
    /usr/bin/python3 hardware/pcb-pi4-controls/build.py --force

The board replaces the printed ButtonCarrier of hardware/enclosure-pi4: same
1.6 mm thickness, same two screw holes, same switch seating plane, widened to
carry the 5 mm RGB LED, J1 and R1-R3. Every position comes from that case
model: dimensions.json (inputs) and output/layout.json (computed), so run the
case build first.

Writes the KiCad project, then runs kicad-cli for ERC, DRC, netlist and plots
into output/, then runs verify.py. Without --force it refuses to overwrite an
existing schematic/board, so hand edits made in KiCad are not lost by accident.

Coordinates: board millimetres with the origin on the switch centre, KiCad axes
(x right, y DOWN) seen from the FRONT side, which faces the lid. Case
coordinates (X right, Y up toward the top wall, seen from the front of the
case) map as X = 18.5 + x, Y = 105.75 - y.
"""
import argparse
import json
import re
import subprocess
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
CASE = HERE.parent / "enclosure-pi4"
NAME = "pi4-controls"
LIBNAME = "PiControls"
NS = uuid.UUID("0c3f6f8e-51a2-4f0e-8d7b-4b1e2f9a7c21")
KICAD_LIB = Path.home() / ".local/share/kicad/10.0/official-10.0.6"
REV, DATE = "0.4 DRAFT", "2026-11-17"
# SW1 terminal holes: round, plated, finished size. Omron's B3F catalogue gives
# 4 x 1.2 +-0.05 mm on 12.5 x 5.0 mm for the B3F-4055; 1.1 mm is a little snugger
# so the kinked legs snap in as they do in 0.1" perfboard. The 1.0 x 0.3 mm leg
# needs 1.044 mm across its corners, and OSH Park holds finished holes to
# +-0.025 mm typical (+-0.064 mm worst case), so 1.1 mm still lets every leg in.
SW_HOLE = 1.1

C = json.loads((CASE / "dimensions.json").read_text())
LAY = json.loads((CASE / "output" / "layout.json").read_text())
BX, BY = LAY["button_centre"]


def uid(key):
    return str(uuid.uuid5(NS, key))


def q(value):
    return json.dumps(str(value))


def b(X, Y):
    """Case (X, Y) -> board (x, y)."""
    return (round(X - BX, 3), round(BY - Y, 3))


# ---------------------------------------------------------------- design data
# Six adjacent pins of the Pi's inner (odd) column, physical 1-11, so one
# straight 6-wire ribbon with a 1x6 housing at each end connects pin n to pin n.
J1_PINS = [  # (J1 pin, net, Pi physical pin, label, GPIO)
    ("1", "+3V3", 1, "3V3", ""),
    ("2", "BTN", 3, "BTN", "GPIO2"),
    ("3", "LED_B", 5, "LED_B", "GPIO3"),
    ("4", "LED_R", 7, "LED_R", "GPIO4"),
    ("5", "GND", 9, "GND", ""),
    ("6", "LED_G", 11, "LED_G", "GPIO17"),
]
R_STOCK = "R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal"
R_FP = R_STOCK + "_Stacked2.8"     # same pads; silk and courtyard hug the 2.5 mm body so rows can be 2.8 mm apart
PARTS = {
    "J1": dict(sym="Conn_01x06", value="Pi header 1x6", fp="PinHeader_1x06_P2.54mm_Vertical",
               nets={p: n for p, n, *_ in J1_PINS}),
    # LED_BGAR: pin 1 blue K, 2 green K, 3 common anode, 4 red K.
    "D1": dict(sym="LED_BGAR", value="EBOOT 5mm RGB common anode", fp="LED_D5.0mm-4_RGB_P2.0mm_Splayed",
               nets={"1": "LED_B_K", "2": "LED_G_K", "3": "+3V3", "4": "LED_R_K"}),
    "R1": dict(sym="R", value="330", fp=R_FP, nets={"1": "LED_B", "2": "LED_B_K"}),
    "R2": dict(sym="R", value="330", fp=R_FP, nets={"1": "LED_G", "2": "LED_G_K"}),
    "R3": dict(sym="R", value="330", fp=R_FP, nets={"1": "LED_R", "2": "LED_R_K"}),
    "SW1": dict(sym="SW_Push", value="Omron B3F-4055", fp="SW_Omron_B3F-4055_PCB", nets={"1": "BTN", "2": "GND"}),
}
STOCK_FOOTPRINTS = [
    ("Connector_PinHeader_2.54mm", "PinHeader_1x06_P2.54mm_Vertical"),
    ("Resistor_THT", R_STOCK),
]
STOCK_SYMBOLS = [
    ("Device", "R"), ("Device", "LED_BGAR"), ("Switch", "SW_Push"),
    ("Connector_Generic", "Conn_01x06"),
    ("power", "+3V3"), ("power", "GND"), ("power", "PWR_FLAG"),
]

# ------------------------------------------------------------ custom footprints
LED_FP = '''(footprint "LED_D5.0mm-4_RGB_P2.0mm_Splayed"
	(version 20250114)
	(generator "pi4_controls_build")
	(layer "F.Cu")
	(descr "5 mm 4-lead RGB LED, common anode, EBOOT 'Multicolor 02' (see reference/eboot-rgb-led-5mm-spec.png). Pads at 2.0 mm pitch so the leads (0.5 mm, about 1 mm apart at the body) can be splayed slightly for hand soldering. Order: R, A(+), G, B. Derived from KiCad LED_D5.0mm-4_RGB.")
	(tags "LED RGB 5mm common anode")
	(property "Reference" "REF**" (at 0 -4.2 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.12))))
	(property "Value" "LED_RGB_5mm" (at 0 4.9 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
	(attr through_hole)
	(fp_circle (center 0 0) (end 2.5 0) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
	(fp_circle (center 0 0) (end 2.9 0) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
	(fp_arc (start -2.12 -2.53) (mid 0 -3.3) (end 2.12 -2.53) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
	(fp_circle (center 0 0) (end 3.5 0) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
	(fp_text user "R" (at -3 2.05 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15))))
	(fp_text user "+" (at -1 2.05 0) (layer "F.SilkS") (effects (font (size 0.9 0.9) (thickness 0.18))))
	(fp_text user "G" (at 1 2.05 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15))))
	(fp_text user "B" (at 3 2.05 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15))))
	(fp_text user "R" (at -3 2.05 0) (layer "B.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15)) (justify mirror)))
	(fp_text user "+" (at -1 2.05 0) (layer "B.SilkS") (effects (font (size 0.9 0.9) (thickness 0.18)) (justify mirror)))
	(fp_text user "G" (at 1 2.05 0) (layer "B.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15)) (justify mirror)))
	(fp_text user "B" (at 3 2.05 0) (layer "B.SilkS") (effects (font (size 0.8 0.8) (thickness 0.15)) (justify mirror)))
	(pad "1" thru_hole oval (at 3 0) (size 1.3 2.2) (drill 0.9) (layers "*.Cu" "*.Mask"))
	(pad "2" thru_hole oval (at 1 0) (size 1.3 2.2) (drill 0.9) (layers "*.Cu" "*.Mask"))
	(pad "3" thru_hole rect (at -1 0) (size 1.3 2.2) (drill 0.9) (layers "*.Cu" "*.Mask"))
	(pad "4" thru_hole oval (at -3 0) (size 1.3 2.2) (drill 0.9) (layers "*.Cu" "*.Mask"))
	(model "${KIPRJMOD}/libraries/PiControls.3dshapes/LED_5mm_RGB_standoff.wrl"
		(offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
)
'''


def switch_fp():
    """Omron B3F-4055 on the board that replaces the printed carrier. Terminal
    holes are round SW_HOLE mm plated holes (a snap fit, see SW_HOLE); the pads
    keep the 3.2 x 3.4 mm size they had around the carrier's 2.0 x 2.2 mm slots,
    and the boss holes stay at the carrier's 2.0 mm. Only the
    two diagonal terminals are wired (pad 1 BTN, pad 2 GND): a diagonal pair is
    always across the switch whichever way the terminals are paired inside, and
    the other two are plated but unnumbered (mechanical)."""
    sx, sy = C["carrier_slot"]
    hw, hn = C["switch_terminal_pitch_wide"] / 2, C["switch_terminal_pitch_narrow"] / 2
    boss = C["switch_boss_spacing"] / 2
    pads = []
    for num, x, y in (("1", hw, hn), ("2", -hw, -hn), ("", -hw, hn), ("", hw, -hn)):
        pads.append(f'\t(pad "{num}" thru_hole oval (at {x} {y}) (size {sx + 1.2} {sy + 1.2}) (drill {SW_HOLE}) '
                    f'(layers "*.Cu" "*.Mask"))')
    for y in (-boss, boss):
        h = C["carrier_boss_hole"]
        pads.append(f'\t(pad "" np_thru_hole circle (at 0 {y}) (size {h} {h}) (drill {h}) (layers "*.Cu" "*.Mask"))')
    s = C["switch_body"] / 2
    silk = [((-s, -s), (s, -s)), ((-s, s), (s, s)), ((-s, -s), (-s, -4.7)), ((-s, s), (-s, 4.7)),
            ((s, -s), (s, -4.7)), ((s, s), (s, 4.7))]
    lines = "\n".join(f'\t(fp_line (start {a[0]} {a[1]}) (end {c[0]} {c[1]}) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))'
                      for a, c in silk)
    return f'''(footprint "SW_Omron_B3F-4055_PCB"
	(version 20250114)
	(generator "pi4_controls_build")
	(layer "F.Cu")
	(descr "Omron B3F-4055 12 x 12 mm tactile switch. Round {SW_HOLE} mm plated terminal holes on 12.5 x 5.0 mm for a snap fit (Omron recommends 1.2 mm); boss holes {C['carrier_boss_hole']} mm as in the printed ButtonCarrier (hardware/enclosure-pi4). Pads 1 and 2 are a diagonal pair.")
	(tags "switch tactile 12mm B3F")
	(property "Reference" "REF**" (at 0 -1.2 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.12))))
	(property "Value" "B3F-4055" (at 0 0.4 0) (layer "F.Fab") (effects (font (size 0.8 0.8) (thickness 0.12))))
	(attr through_hole)
	(fp_rect (start {-s} {-s}) (end {s} {s}) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
	(fp_circle (center 0 0) (end 2.9 0) (stroke (width 0.12) (type solid)) (fill no) (layer "F.SilkS"))
{lines}
	(fp_rect (start {-hw - sx / 2 - 0.85} {-s - 0.25}) (end {hw + sx / 2 + 0.85} {s + 0.25}) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
{chr(10).join(pads)}
	(model "${{KIPRJMOD}}/libraries/PiControls.3dshapes/SW_Omron_B3F-4055.wrl"
		(offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
)
'''


def hole_fp():
    h = C["pcb_hole"]
    post = C["camera_post_diameter"] / 2
    return f'''(footprint "MountingHole_{h}mm_M2_LidPost"
	(version 20250114)
	(generator "pi4_controls_build")
	(layer "F.Cu")
	(descr "M2 x 6 screw from the back into a {C['camera_post_diameter']} mm lid post. {h} mm NPTH as in the printed ButtonCarrier. Front courtyard is the post, back courtyard and silk the screw head.")
	(property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (hide yes) (effects (font (size 0.8 0.8) (thickness 0.12))))
	(property "Value" "M2" (at 0 3 0) (layer "F.Fab") (effects (font (size 0.8 0.8) (thickness 0.12))))
	(attr board_only exclude_from_pos_files exclude_from_bom)
	(fp_circle (center 0 0) (end {post} 0) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
	(fp_circle (center 0 0) (end {post} 0) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))
	(fp_circle (center 0 0) (end 2.0 0) (stroke (width 0.12) (type solid)) (fill no) (layer "B.SilkS"))
	(fp_circle (center 0 0) (end 2.1 0) (stroke (width 0.05) (type solid)) (fill no) (layer "B.CrtYd"))
	(pad "" np_thru_hole circle (at 0 0) (size {h} {h}) (drill {h}) (layers "*.Cu" "*.Mask"))
)
'''


def sexpr_block(text, start):
    depth, string, escape = 0, False, False
    for i in range(start, len(text)):
        c = text[i]
        if escape:
            escape = False
        elif string and c == "\\":
            escape = True
        elif c == '"':
            string = not string
        elif not string:
            depth += (c == "(") - (c == ")")
            if depth == 0:
                return text[start:i + 1]
    raise ValueError("unbalanced")


def symbol_block(text, name):
    return sexpr_block(text, text.index('(symbol "' + name + '"'))


def vendor(kicad_lib):
    """Copy the stock parts into libraries/ (done once; the copies are the inputs)."""
    lib = HERE / "libraries"
    pretty = lib / f"{LIBNAME}.pretty"
    pretty.mkdir(parents=True, exist_ok=True)
    for libname, fp in STOCK_FOOTPRINTS:
        text = (kicad_lib / "footprints" / f"{libname}.pretty" / f"{fp}.kicad_mod").read_text()
        (pretty / f"{fp}.kicad_mod").write_text(text)
    blocks = []
    for libname, sym in STOCK_SYMBOLS:
        text = (kicad_lib / "symbols" / f"{libname}.kicad_sym").read_text()
        blocks.append("\t" + symbol_block(text, sym))
    (lib / f"{LIBNAME}.kicad_sym").write_text(
        '(kicad_symbol_lib\n\t(version 20251024)\n\t(generator "pi4_controls_build")\n'
        + "\n".join(blocks) + "\n)\n")


def write_custom_footprints():
    pretty = HERE / "libraries" / f"{LIBNAME}.pretty"
    (pretty / "LED_D5.0mm-4_RGB_P2.0mm_Splayed.kicad_mod").write_text(LED_FP)
    (pretty / "SW_Omron_B3F-4055_PCB.kicad_mod").write_text(switch_fp())
    (pretty / f"MountingHole_{C['pcb_hole']}mm_M2_LidPost.kicad_mod").write_text(hole_fp())
    r = (pretty / f"{R_STOCK}.kicad_mod").read_text()
    for old, new in (('-1.37)', '-1.3)'), (' 1.37)', ' 1.3)'), ('(start -1.05 -1.5)', '(start -1.05 -1.38)'),
                     ('(end 8.67 1.5)', '(end 8.67 1.38)'), (f'(footprint "{R_STOCK}"', f'(footprint "{R_FP}"')):
        assert old in r, old
        r = r.replace(old, new)
    (pretty / f"{R_FP}.kicad_mod").write_text(r)


# ------------------------------------------------------------------ schematic
def pins_of(block):
    found = re.findall(r'\(pin \w+ \w+\s*\(at ([-\d.]+) ([-\d.]+) (\d+)\)[\s\S]*?\(number "(\w+)"', block)
    return {n: (float(x), float(y), int(a)) for x, y, a, n in found}


def build_schematic(root_id):
    libtext = (HERE / "libraries" / f"{LIBNAME}.kicad_sym").read_text()
    syms = {name: symbol_block(libtext, name) for _, name in STOCK_SYMBOLS}
    lib_symbols = "".join(
        blk.replace(f'(symbol "{n}"', f'(symbol "{LIBNAME}:{n}"', 1) for n, blk in syms.items())
    items = []
    pwr = [0]

    def rot(x, y, a):
        a %= 360
        return {0: (x, y), 90: (-y, x), 180: (-x, -y), 270: (y, -x)}[a]

    def pin_at(sym, at, angle, num):
        px, py, pa = pins_of(syms[sym])[num]
        rx, ry = rot(px, py, angle)
        return (round(at[0] + rx, 2), round(at[1] - ry, 2)), (pa + angle + 180) % 360

    def wire(*pts):
        for a, c in zip(pts, pts[1:]):
            items.append(f'(wire (pts (xy {a[0]} {a[1]}) (xy {c[0]} {c[1]})) (stroke (width 0) (type default)) (uuid {uid(str((a, c)))}))')

    def junction(p):
        items.append(f'(junction (at {p[0]} {p[1]}) (diameter 0) (color 0 0 0 0) (uuid {uid("j" + str(p))}))')

    def label(net, p, angle):
        just = "right bottom" if angle == 180 else "left bottom"
        items.append(f'(label {q(net)} (at {p[0]} {p[1]} {angle}) (effects (font (size 1.27 1.27)) (justify {just})) (uuid {uid(str((net, p)))}))')

    def note(text, x, y, size=1.27):
        items.append(f'(text {q(text)} (exclude_from_sim no) (at {x} {y} 0) (effects (font (size {size} {size})) (justify left top)) (uuid {uid(text)}))')

    def symbol(sym, ref, value, at, angle=0, footprint="", ref_at=None, val_at=None, hide_ref=False, fangle=0):
        x, y = at
        rx, ry = ref_at or (x, y - 5)
        vx, vy = val_at or (x, y + 5)
        fpprop = f"{LIBNAME}:{footprint}" if footprint else ""
        power = ref.startswith("#")
        items.append(f'''(symbol (lib_id "{LIBNAME}:{sym}") (at {x} {y} {angle}) (unit 1)
  (exclude_from_sim no) (in_bom {"no" if power else "yes"}) (on_board {"no" if power else "yes"}) (dnp no) (uuid {uid(ref)})
  (property "Reference" {q(ref)} (at {rx} {ry} {fangle}) (effects (font (size 1.27 1.27)) {"(hide yes)" if hide_ref or power else ""}))
  (property "Value" {q(value)} (at {vx} {vy} {fangle}) (effects (font (size 1.27 1.27))))
  (property "Footprint" {q(fpprop)} (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
  (property "Datasheet" "" (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
  (instances (project {q(NAME)} (path "/{root_id}" (reference {q(ref)}) (unit 1)))))''')

    def power(kind, p, outward):
        pwr[0] += 1
        ref = ("#FLG%02d" if kind == "PWR_FLAG" else "#PWR%02d") % pwr[0]
        body = {"+3V3": 90, "GND": 270, "PWR_FLAG": 90}[kind]
        angle = (outward - body) % 360
        off = {0: (4, 0), 90: (0, -4), 180: (-6, 0), 270: (0, 5)}[outward]
        symbol(kind, ref, kind, p, angle, val_at=(p[0] + off[0], p[1] + off[1]))

    def stub(p, outward, length=5.08):
        dx, dy = {0: (length, 0), 90: (0, -length), 180: (-length, 0), 270: (0, length)}[outward]
        end = (round(p[0] + dx, 2), round(p[1] + dy, 2))
        wire(p, end)
        return end

    j1 = (63.5, 88.9)
    symbol("Conn_01x06", "J1", PARTS["J1"]["value"], j1, footprint=PARTS["J1"]["fp"],
           ref_at=(j1[0] - 2, j1[1] - 9), val_at=(j1[0] - 2, j1[1] + 12))
    for pin, net, pi_pin, _, gpio in J1_PINS:
        p, out = pin_at("Conn_01x06", j1, 0, pin)
        if net == "+3V3":
            corner = stub(p, out)
            wire(corner, (corner[0], corner[1] - 5.08))
            power("+3V3", (corner[0], corner[1] - 5.08), 90)
            flag = stub(corner, 180, 5.08)
            power("PWR_FLAG", flag, 90)
            junction(corner)
        elif net == "GND":
            end = stub(p, out, 17.78)
            power("GND", end, 270)
        else:
            label(net, stub(p, out), 180)
        note(f"Pi pin {pi_pin}" + (f" / {gpio}" if gpio else ""), j1[0] + 3.5, p[1] - 0.9, 1.1)
    note("J1 = 1x6 2.54 mm straight header on the back of the board. Straight 6-wire ribbon, one 1x6 housing\n"
         "at each end, onto the Pi's inner column pins 1-11 (pin 1 corner first): J1 pin n -> Pi pin 2n-1.\n"
         "1 3V3 -> Pi 1   2 BTN -> Pi 3 (GPIO 2)   3 LED_B -> Pi 5 (GPIO 3)\n"
         "4 LED_R -> Pi 7 (GPIO 4)   5 GND -> Pi 9   6 LED_G -> Pi 11 (GPIO 17)\n"
         "Keep I2C (GPIO 2/3) and 1-Wire (GPIO 4) disabled on this Pi.",
         30.48, 109.22, 1.2)

    d1 = (165.1, 88.9)
    symbol("LED_BGAR", "D1", PARTS["D1"]["value"], d1, footprint=PARTS["D1"]["fp"],
           ref_at=(d1[0] - 2, d1[1] - 10), val_at=(d1[0] - 12, d1[1] + 10))
    for ref, dpin in (("R3", "4"), ("R2", "2"), ("R1", "1")):
        dp, _ = pin_at("LED_BGAR", d1, 0, dpin)
        at = (dp[0] - 17.78, dp[1])
        symbol("R", ref, "330", at, 90, footprint=PARTS[ref]["fp"],
               ref_at=(at[0] - 2.5, at[1] - 2.2), val_at=(at[0] + 2.5, at[1] - 2.2), fangle=90)
        p1, o1 = pin_at("R", at, 90, "1")
        p2, _ = pin_at("R", at, 90, "2")
        wire(p2, dp)
        label(PARTS[ref]["nets"]["2"], (round(p2[0] + 2.54, 2), p2[1]), 0)
        label(PARTS[ref]["nets"]["1"], stub(p1, o1), 180)
    ap, ao = pin_at("LED_BGAR", d1, 0, "3")
    corner = stub(ap, ao)
    wire(corner, (corner[0], corner[1] - 5.08))
    power("+3V3", (corner[0], corner[1] - 5.08), 90)
    note("D1: EBOOT 5 mm clear RGB, COMMON ANODE.\n"
         "Legs: 1 blue, 2 green, 3 common anode (longest), 4 red.\n"
         "Seen from the front, red on one end: R, +, G, B.\n"
         "330R on each colour, none on the common. GPIO LOW = colour on.",
         137.16, 104.14, 1.2)

    sw = (165.1, 135.89)
    symbol("SW_Push", "SW1", PARTS["SW1"]["value"], sw, footprint=PARTS["SW1"]["fp"],
           ref_at=(sw[0], sw[1] - 4.5), val_at=(sw[0] - 8, sw[1] + 4))
    p1, o1 = pin_at("SW_Push", sw, 0, "1")
    p2, o2 = pin_at("SW_Push", sw, 0, "2")
    label("BTN", stub(p1, o1), 180)
    corner = stub(p2, o2)
    wire(corner, (corner[0], corner[1] + 5.08))
    power("GND", (corner[0], corner[1] + 5.08), 270)
    flag = stub(corner, 0, 5.08)
    power("PWR_FLAG", flag, 90)
    junction(corner)
    note("SW1: Omron B3F-4055, BTN to GND. No pull-up fitted:\n"
         "GPIO 2 (Pi pin 3) has an on-board pull-up.\n"
         "Wired on a diagonal pair of terminals, so it works\n"
         "whichever way the four terminals are paired inside.",
         137.16, 152.4, 1.2)

    note("DAILY MIRROR - Pi 4 CASE CONTROL BOARD", 25.4, 30.48, 2.5)
    note(f"Draft v{REV.split()[0]} for review; generated by build.py. Replaces the printed ButtonCarrier of hardware/enclosure-pi4. See README.md.",
         25.4, 36.83, 1.4)

    sch = f'''(kicad_sch (version 20250114) (generator "pi4_controls_build") (generator_version "10.0")
  (uuid {root_id}) (paper "A4")
  (title_block (title "Daily Mirror - Pi 4 case control board") (date "{DATE}") (rev "{REV}")
    (company "Daily Mirror") (comment 1 "Draft for review - not order-ready"))
  (lib_symbols {lib_symbols})
  {chr(10).join(items)}
  (sheet_instances (path "/" (page "1"))) (embedded_fonts no))
'''
    (HERE / f"{NAME}.kicad_sch").write_text(sch)


# ---------------------------------------------------------------------- board
OX, OY = 100.0, 100.0          # page position of the switch centre
TRACK = 0.5

# Placement in CASE coordinates (X right, Y up, seen from the front).
X0, Y0, X1, Y1 = C["pcb_outline"]
LED = (C["led_x"], LAY["led_centre"][1])
HOLES = {f"H{i + 1}": tuple(p) for i, p in enumerate(LAY["control_pcb"]["holes"])}
J1_AT = tuple(LAY["control_pcb"]["j1_pins"][0])
RX0, RX1 = C["resistor_x"]
R_ROWS = dict(zip(("R2", "R3", "R1"), C["resistor_rows_y"]))   # green nearest J1, then red, blue

# Tracks, case coordinates, through the gaps worked out in README "Routing".
D = C["led_lead_spacing"]
LY = LED[1]
# Column, top to bottom: B, G, +, R (red at the bottom end, toward the Pi).
PAD = {"R": (LED[0], LY - 1.5 * D), "A": (LED[0], LY - .5 * D), "G": (LED[0], LY + .5 * D), "B": (LED[0], LY + 1.5 * D)}
SW_BTN = (BX + 6.25, BY - 2.5)          # pad 1, lower right seen from the front
SW_GND = (BX - 6.25, BY + 2.5)          # pad 2, upper left
J = [(J1_AT[0], J1_AT[1] + 2.54 * i) for i in range(6)]
ROUTES = [
    # net, layer, points (case X, Y)
    ("+3V3", "F", [J[0], (7.0, 94.2), (27.2, 94.2), (27.2, PAD["A"][1]), PAD["A"]]),
    ("BTN", "F", [J[1], (SW_BTN[0], J[1][1]), SW_BTN]),
    ("GND", "F", [J[4], (8.8, J[4][1]), (8.8, SW_GND[1]), SW_GND]),
    ("LED_G_K", "F", [(RX1, R_ROWS["R2"]), (27.7, R_ROWS["R2"]), (27.7, PAD["G"][1]), PAD["G"]]),
    ("LED_B_K", "F", [(RX1, R_ROWS["R1"]), (31.5, R_ROWS["R1"]), (33.0, R_ROWS["R1"] - 1.5), (33.0, PAD["B"][1]), PAD["B"]]),
    ("LED_G", "B", [J[5], (RX0, R_ROWS["R2"])]),
    ("LED_R", "B", [J[3], (8.0, J[3][1]), (8.0, R_ROWS["R3"]), (RX0, R_ROWS["R3"])]),
    ("LED_B", "B", [J[2], (9.5, J[2][1]), (9.5, R_ROWS["R1"]), (RX0, R_ROWS["R1"])]),
    ("LED_R_K", "B", [(RX1, R_ROWS["R3"]), (15.6, R_ROWS["R3"]), (15.6, 112.0), (28.0, 112.0), (28.0, PAD["R"][1]), PAD["R"]]),
]


def build_board(root_id):
    import pcbnew as pcb

    mm = pcb.FromMM

    def pt(x, y):
        return pcb.VECTOR2I(mm(OX + x), mm(OY + y))

    def P(X, Y):
        return pt(*b(X, Y))

    board = pcb.BOARD()
    board.SetCopperLayerCount(2)
    ds = board.GetDesignSettings()
    ds.SetBoardThickness(mm(C["carrier_thickness"]))
    ds.SetAuxOrigin(pt(0, 0))
    ds.SetGridOrigin(pt(0, 0))
    tb = board.GetTitleBlock()
    tb.SetTitle("Daily Mirror - Pi 4 case control board")
    tb.SetRevision(REV)
    tb.SetDate(DATE)
    tb.SetComment(0, "Draft for review - not order-ready")
    nets = {}
    for p in PARTS.values():
        for n in p["nets"].values():
            if n not in nets:
                nets[n] = pcb.NETINFO_ITEM(board, n if n in ("+3V3", "GND") else "/" + n)
                board.Add(nets[n])
    lib = str(HERE / "libraries" / f"{LIBNAME}.pretty")

    def place(ref, fpname, X, Y, angle=0, back=False, value=None):
        fp = pcb.FootprintLoad(lib, fpname)
        assert fp is not None, fpname
        fp.SetFPID(pcb.LIB_ID(LIBNAME, fpname))
        fp.SetReference(ref)
        fp.SetValue(value or PARTS.get(ref, {}).get("value", ""))
        fp.SetPosition(P(X, Y))
        fp.SetOrientationDegrees(angle)
        fp.Value().SetVisible(False)
        board.Add(fp)
        if back:
            fp.Flip(fp.GetPosition(), pcb.FLIP_DIRECTION_LEFT_RIGHT)
        if ref in PARTS:
            fp.SetPath(pcb.KIID_PATH(f"/{root_id}/{uid(ref)}"))
            for pad in fp.Pads():
                if pad.GetNumber():
                    pad.SetNet(nets[PARTS[ref]["nets"][pad.GetNumber()]])
        return fp

    def ref_text(fp, X, Y, size=0.8, angle=None):
        fp.Reference().SetPosition(P(X, Y))
        fp.Reference().SetTextSize(pcb.VECTOR2I(mm(size), mm(size)))
        fp.Reference().SetTextThickness(mm(0.12))
        if angle is not None:
            fp.Reference().SetTextAngleDegrees(angle)

    fps = {}
    fps["SW1"] = place("SW1", PARTS["SW1"]["fp"], BX, BY)
    fps["D1"] = place("D1", PARTS["D1"]["fp"], *LED, angle=90)       # red pad toward the Pi (bottom)
    ref_text(fps["D1"], LED[0] + 2.3, LED[1] + 4.6)
    fps["J1"] = place("J1", PARTS["J1"]["fp"], *J1_AT, angle=180, back=True)
    ref_text(fps["J1"], J1_AT[0] + 2.3, J1_AT[1] - 1.6)
    for ref, y in R_ROWS.items():
        fps[ref] = place(ref, R_FP, RX0, y, angle=180, back=True)
        ref_text(fps[ref], (RX0 + RX1) / 2, y, 0.8)
    for ref, (X, Y) in HOLES.items():
        fp = place(ref, f"MountingHole_{C['pcb_hole']}mm_M2_LidPost", X, Y, value="M2")
        fp.Reference().SetVisible(False)

    def pad(ref, num):
        pads = [p for p in fps[ref].Pads() if p.GetNumber() == num]
        assert len(pads) == 1, (ref, num)
        v = pads[0].GetPosition()
        return (round(pcb.ToMM(v.x) - OX + BX, 3), round(BY - (pcb.ToMM(v.y) - OY), 3))

    # Footprint orientation is what the routing assumes (case coordinates).
    close = lambda a, c: abs(a[0] - c[0]) < 1e-3 and abs(a[1] - c[1]) < 1e-3
    for i in range(6):
        assert close(pad("J1", str(i + 1)), J[i]), ("J1", i + 1, pad("J1", str(i + 1)))
    for ref, y in R_ROWS.items():
        assert close(pad(ref, "1"), (RX0, y)) and close(pad(ref, "2"), (RX1, y)), (ref, pad(ref, "1"), pad(ref, "2"))
    for num, key in (("4", "R"), ("3", "A"), ("2", "G"), ("1", "B")):
        assert close(pad("D1", num), PAD[key]), ("D1", num, pad("D1", num), PAD[key])
    assert close(pad("SW1", "1"), SW_BTN) and close(pad("SW1", "2"), SW_GND), (pad("SW1", "1"), pad("SW1", "2"))

    layers = {"F": pcb.F_Cu, "B": pcb.B_Cu}
    for net, layer, pts in ROUTES:
        for a, c in zip(pts, pts[1:]):
            t = pcb.PCB_TRACK(board)
            t.SetStart(P(*a))
            t.SetEnd(P(*c))
            t.SetWidth(mm(TRACK))
            t.SetLayer(layers[layer])
            t.SetNet(nets[net])
            board.Add(t)

    # Outline: rectangle with rounded corners, as the printed carrier.
    r = C["pcb_corner_radius"]

    def seg(a, c):
        s = pcb.PCB_SHAPE(board)
        s.SetShape(pcb.SHAPE_T_SEGMENT)
        s.SetStart(P(*a))
        s.SetEnd(P(*c))
        s.SetLayer(pcb.Edge_Cuts)
        s.SetWidth(mm(0.1))
        board.Add(s)

    def arc(cx, cy, a0):
        import math
        s = pcb.PCB_SHAPE(board)
        s.SetShape(pcb.SHAPE_T_ARC)
        pts = [(cx + r * math.cos(math.radians(a)), cy + r * math.sin(math.radians(a))) for a in (a0, a0 + 45, a0 + 90)]
        s.SetArcGeometry(P(*pts[0]), P(*pts[1]), P(*pts[2]))
        s.SetLayer(pcb.Edge_Cuts)
        s.SetWidth(mm(0.1))
        board.Add(s)

    seg((X0 + r, Y0), (X1 - r, Y0))
    seg((X1, Y0 + r), (X1, Y1 - r))
    seg((X1 - r, Y1), (X0 + r, Y1))
    seg((X0, Y1 - r), (X0, Y0 + r))
    arc(X1 - r, Y0 + r, 270)
    arc(X1 - r, Y1 - r, 0)
    arc(X0 + r, Y1 - r, 90)
    arc(X0 + r, Y0 + r, 180)

    def text(value, X, Y, size=0.8, layer=pcb.F_SilkS, angle=0, left=False):
        t = pcb.PCB_TEXT(board)
        t.SetText(value)
        t.SetPosition(P(X, Y))
        t.SetTextSize(pcb.VECTOR2I(mm(size), mm(size)))
        t.SetTextThickness(mm(max(0.12, size * 0.15)))
        t.SetLayer(layer)
        t.SetTextAngleDegrees(angle)
        if left:
            t.SetHorizJustify(pcb.GR_TEXT_H_ALIGN_LEFT)
        if layer in (pcb.B_SilkS, pcb.B_Fab):
            t.SetMirrored(True)
        board.Add(t)

    # Front (lid side).
    text(f"D1 FLANGE {LAY['led_standoff_above_pcb']} mm ABOVE BOARD", 7.6, 106.6, 0.7, angle=90)
    text("LID SIDE", 30.2, 99.6, 0.7)
    # Back: the J1 table under the switch, between its terminals; read from the back.
    bs = pcb.B_SilkS
    for i, (pin, _net, pi_pin, lab, _g) in enumerate(J1_PINS):
        text(f"{pin} {lab:<5} Pi {pi_pin}", BX, BY + 2.85 - 1.15 * i, 0.7, bs)
    text("1", J1_AT[0] + 2.0, J1_AT[1], 0.8, bs)
    text("6", J[5][0] + 2.0, J[5][1], 0.8, bs)
    text("330R", (RX0 + RX1) / 2, min(R_ROWS.values()) - 2.15, 0.7, bs)
    text("DAILY MIRROR", 24.5, 98.6, 0.6, bs)
    text(f"PI4 v{REV.split()[0]}", 24.5, 97.4, 0.6, bs)

    pcb.SaveBoard(str(HERE / f"{NAME}.kicad_pcb"), board)


def write_project(root_id):
    pro = {
        "meta": {"filename": f"{NAME}.kicad_pro", "version": 3},
        "board": {"design_settings": {
            "defaults": {"board_outline_line_width": 0.1},
            "rules": {"min_clearance": 0.2, "min_track_width": 0.25, "min_copper_edge_clearance": 0.5,
                      "min_hole_clearance": 0.25, "min_through_hole_diameter": 0.3,
                      "min_silk_clearance": 0.0, "min_text_height": 0.6, "min_text_thickness": 0.1},
            "track_widths": [0.0, 0.5, 0.8], "via_dimensions": [], "drc_exclusions": []}},
        "net_settings": {"meta": {"version": 4}, "classes": [{
            "name": "Default", "clearance": 0.25, "track_width": 0.5,
            "via_diameter": 0.8, "via_drill": 0.4, "diff_pair_width": 0.2, "diff_pair_gap": 0.25,
            "diff_pair_via_gap": 0.25, "microvia_diameter": 0.3, "microvia_drill": 0.1,
            "wire_width": 6, "bus_width": 12, "line_style": 0, "pcb_color": "rgba(0, 0, 0, 0.000)",
            "schematic_color": "rgba(0, 0, 0, 0.000)", "priority": 2147483647}]},
        "text_variables": {"DESIGN_STATUS": "DRAFT - NOT ORDER-READY"},
        "libraries": {"pinned_symbol_libs": [LIBNAME], "pinned_footprint_libs": [LIBNAME]},
        "sheets": [[root_id, "Root"]],
    }
    (HERE / f"{NAME}.kicad_pro").write_text(json.dumps(pro, indent=2) + "\n")
    (HERE / "sym-lib-table").write_text(
        f'(sym_lib_table (version 7) (lib (name "{LIBNAME}") (type "KiCad") (uri "${{KIPRJMOD}}/libraries/{LIBNAME}.kicad_sym") (options "") (descr "Vendored KiCad 10.0.6 symbols")))\n')
    (HERE / "fp-lib-table").write_text(
        f'(fp_lib_table (version 7) (lib (name "{LIBNAME}") (type "KiCad") (uri "${{KIPRJMOD}}/libraries/{LIBNAME}.pretty") (options "") (descr "Vendored KiCad 10.0.6 footprints + custom LED, switch, holes")))\n')


# -------------------------------------------------------------------- outputs
def run(*args, env=None):
    print("+", " ".join(str(a) for a in args))
    return subprocess.run([str(a) for a in args], check=False, env=env)


def outputs(kicad_lib):
    import os
    out = HERE / "output"
    out.mkdir(exist_ok=True)
    sch, pcbf = HERE / f"{NAME}.kicad_sch", HERE / f"{NAME}.kicad_pcb"
    env = dict(os.environ, KICAD10_3DMODEL_DIR=str(kicad_lib / "3dmodels"))
    run("kicad-cli", "sch", "erc", "--format", "json", "--severity-all", "-o", out / "erc.json", sch)
    run("kicad-cli", "sch", "erc", "--severity-all", "-o", out / "erc.rpt", sch)
    run("kicad-cli", "pcb", "drc", "--format", "json", "--severity-all", "--schematic-parity",
        "-o", out / "drc.json", pcbf)
    run("kicad-cli", "pcb", "drc", "--severity-all", "--schematic-parity", "-o", out / "drc.rpt", pcbf)
    run("kicad-cli", "sch", "export", "netlist", "--format", "kicadxml", "-o", out / "netlist.xml", sch)
    run("kicad-cli", "sch", "export", "pdf", "-o", out / "schematic.pdf", sch)
    common = ["--mode-single", "--page-size-mode", "2", "--exclude-drawing-sheet", "--drill-shape-opt", "2"]
    run("kicad-cli", "pcb", "export", "svg", *common, "-l", "Edge.Cuts,F.Cu,F.Mask,F.SilkS,F.Fab",
        "-o", out / "board-front.svg", pcbf)
    run("kicad-cli", "pcb", "export", "svg", *common, "--mirror", "-l", "Edge.Cuts,B.Cu,B.Mask,B.SilkS",
        "-o", out / "board-back.svg", pcbf)
    for side, name in (("top", "front"), ("bottom", "back")):
        run("kicad-cli", "pcb", "render", "--side", side, "--quality", "high", "-w", "1400", "-h", "1300",
            "--background", "opaque", "--zoom", "1.1", "-o", out / f"board-{name}-3d.png", pcbf, env=env)
    run("kicad-cli", "pcb", "render", "--quality", "high", "-w", "1600", "-h", "1100", "--background", "opaque",
        "--rotate", "-55,0,-25", "--zoom", "1.0", "-o", out / "board-angle-3d.png", pcbf, env=env)
    run("/usr/bin/python3", HERE / "verify.py")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--force", action="store_true", help="overwrite existing schematic and board")
    ap.add_argument("--vendor", action="store_true", help="re-copy stock parts from the KiCad library dir")
    ap.add_argument("--kicad-lib", type=Path, default=KICAD_LIB)
    ap.add_argument("--no-outputs", action="store_true")
    args = ap.parse_args()
    if (HERE / f"{NAME}.kicad_pcb").exists() and not args.force:
        raise SystemExit("Design already exists; pass --force to regenerate (hand edits will be lost).")
    if args.vendor or not (HERE / "libraries" / f"{LIBNAME}.kicad_sym").exists():
        vendor(args.kicad_lib)
    write_custom_footprints()
    root_id = uid("schematic")
    build_schematic(root_id)
    build_board(root_id)
    write_project(root_id)
    if not args.no_outputs:
        outputs(args.kicad_lib)


if __name__ == "__main__":
    main()
