#!/usr/bin/python3
"""Check the generated board against the schematic netlist, the settled design
and the case model (../enclosure-pi4/output/layout.json).

Run by build.py after kicad-cli has written output/netlist.xml, erc.json, drc.json.
Writes output/validation.json. Exits non-zero on any mismatch.
"""
import collections
import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import pcbnew

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "output"
CASE = ROOT.parent / "enclosure-pi4"
C = json.loads((CASE / "dimensions.json").read_text())
LAY = json.loads((CASE / "output" / "layout.json").read_text())
SW_HOLE = 1.1   # SW1 terminal holes, round finished mm (snap fit; Omron recommends 1.2)

# The settled electrical design, written out independently of build.py.
EXPECTED = {
    "+3V3": {("J1", "1"), ("D1", "3")},                 # Pi pin 1 -> common anode
    "BTN": {("J1", "2"), ("SW1", "1")},                 # Pi pin 3 / GPIO 2
    "GND": {("J1", "5"), ("SW1", "2")},                 # Pi pin 9
    "LED_B": {("J1", "3"), ("R1", "1")},                # Pi pin 5 / GPIO 3
    "LED_R": {("J1", "4"), ("R3", "1")},                # Pi pin 7 / GPIO 4
    "LED_G": {("J1", "6"), ("R2", "1")},                # Pi pin 11 / GPIO 17
    "LED_G_K": {("R2", "2"), ("D1", "2")},              # D1 leg 2 green
    "LED_B_K": {("R1", "2"), ("D1", "1")},              # D1 leg 1 blue
    "LED_R_K": {("R3", "2"), ("D1", "4")},              # D1 leg 4 red
}

netlist = ET.parse(OUT / "netlist.xml")
schematic = {}
for net in netlist.findall(".//nets/net"):
    name = net.get("name").lstrip("/")
    nodes = {(n.get("ref"), n.get("pin")) for n in net.findall("node")}
    if nodes:
        schematic[name] = nodes
values = {c.get("ref"): c.findtext("value") for c in netlist.findall(".//components/comp")}

board = pcbnew.LoadBoard(str(ROOT / "pi4-controls.kicad_pcb"))
physical = collections.defaultdict(set)
for fp in board.GetFootprints():
    if fp.GetAttributes() & pcbnew.FP_BOARD_ONLY:
        continue
    for pad in fp.Pads():
        if pad.GetNumber():
            physical[pad.GetNetname().lstrip("/")].add((fp.GetReference(), pad.GetNumber()))
physical = dict(physical)

errors = []
if schematic != EXPECTED:
    errors.append({"schematic_vs_design": {"schematic": {k: sorted(v) for k, v in schematic.items()}}})
if physical != EXPECTED:
    errors.append({"board_vs_design": {"board": {k: sorted(v) for k, v in physical.items()}}})
for r in ("R1", "R2", "R3"):
    if values.get(r) != "330":
        errors.append(f"{r} value {values.get(r)}")

sides = {fp.GetReference(): ("back" if fp.IsFlipped() else "front") for fp in board.GetFootprints()}
for ref, side in {"D1": "front", "SW1": "front", "J1": "back", "R1": "back", "R2": "back", "R3": "back"}.items():
    if sides[ref] != side:
        errors.append(f"{ref} on {sides[ref]}, expected {side}")

# Positions against the case model. Board origin (page 100, 100) is the switch centre.
BX, BY = LAY["button_centre"]


def case_xy(v):
    return (round(pcbnew.ToMM(v.x) - 100 + BX, 3), round(BY - (pcbnew.ToMM(v.y) - 100), 3))


fps = {fp.GetReference(): fp for fp in board.GetFootprints()}
placement = {}


def expect(name, got, want, tol=0.01):
    placement[name] = dict(case_xy=list(got), expected=list(want))
    if abs(got[0] - want[0]) > tol or abs(got[1] - want[1]) > tol:
        errors.append(f"{name} at {got}, case model has {want}")


expect("SW1 centre", case_xy(fps["SW1"].GetPosition()), LAY["button_centre"])
expect("D1 centre", case_xy(fps["D1"].GetPosition()), LAY["led_centre"])
holes = sorted(case_xy(fps[f"H{i}"].GetPosition()) for i in range(1, 5))
for got, want in zip(holes, sorted(map(tuple, LAY["switch_posts"] + LAY["led_posts"]))):
    expect(f"hole {want}", got, want)
for i, want in enumerate(LAY["control_pcb"]["j1_pins"]):
    pad = [p for p in fps["J1"].Pads() if p.GetNumber() == str(i + 1)][0]
    expect(f"J1 pin {i + 1}", case_xy(pad.GetPosition()), want)
red = [p for p in fps["D1"].Pads() if p.GetNumber() == "4"][0]
if case_xy(red.GetPosition())[1] >= LAY["led_centre"][1]:
    errors.append("D1 red pad is not at the bottom (Pi) end")
for p in fps["SW1"].Pads():
    if p.GetDrillSizeX() and p.GetAttribute() == pcbnew.PAD_ATTRIB_PTH:
        d = (round(pcbnew.ToMM(p.GetDrillSizeX()), 3), round(pcbnew.ToMM(p.GetDrillSizeY()), 3))
        if d != (SW_HOLE, SW_HOLE) or p.GetDrillShape() != pcbnew.PAD_DRILL_SHAPE_CIRCLE:
            errors.append(f"SW1 terminal hole {d}, expected round {SW_HOLE} mm (snap fit)")
bbox = board.GetBoardEdgesBoundingBox()
lo, hi = case_xy(bbox.GetOrigin()), case_xy(bbox.GetEnd())
outline = [lo[0], hi[1], hi[0], lo[1]]
if any(abs(a - b) > 0.06 for a, b in zip(outline, C["pcb_outline"])):
    errors.append(f"board outline {outline}, case model has {C['pcb_outline']}")
if abs(pcbnew.ToMM(board.GetDesignSettings().GetBoardThickness()) - C["carrier_thickness"]) > 1e-6:
    errors.append("board thickness differs from the carrier")

erc = json.loads((OUT / "erc.json").read_text())
drc = json.loads((OUT / "drc.json").read_text())
erc_v = [v for s in erc["sheets"] for v in s["violations"]]
drc_v = drc["violations"]
if erc_v:
    errors.append({"erc": collections.Counter(v["type"] for v in erc_v)})
if drc_v or drc["unconnected_items"] or drc["schematic_parity"]:
    errors.append({"drc": dict(collections.Counter(v["type"] for v in drc_v)),
                   "unconnected": len(drc["unconnected_items"]),
                   "parity": len(drc["schematic_parity"])})

result = {
    "status": "DRAFT - not order-ready; see README assumptions",
    "kicad_version": pcbnew.GetBuildVersion(),
    "board_bbox_mm": [round(pcbnew.ToMM(bbox.GetWidth()), 2), round(pcbnew.ToMM(bbox.GetHeight()), 2)],
    "board_outline_case_xy": outline,
    "copper_layers": board.GetCopperLayerCount(),
    "tracks": len(board.GetTracks()),
    "nets_checked_against_design": sorted(EXPECTED),
    "schematic_matches_design": schematic == EXPECTED,
    "board_matches_design": physical == EXPECTED,
    "placement_vs_case_model": placement,
    "erc_violations": len(erc_v),
    "drc_violations": len(drc_v),
    "drc_unconnected": len(drc["unconnected_items"]),
    "drc_schematic_parity": len(drc["schematic_parity"]),
    "errors": errors,
}
(OUT / "validation.json").write_text(json.dumps(result, indent=2, default=str) + "\n")
print(json.dumps({k: v for k, v in result.items() if k != "placement_vs_case_model"}, indent=2, default=str))
sys.exit(1 if errors else 0)
