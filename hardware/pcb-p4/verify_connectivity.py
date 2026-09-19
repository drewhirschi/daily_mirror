#!/usr/bin/env python3
"""Check KiCad-exported electrical nets against actual board pads and LED wiring.

Run after refreshing output/netlist.xml, output/erc.json and output/drc.json.
This checks the implemented controls only; it is not a manufacturing sign-off.
"""
import json
import collections
import re
from pathlib import Path
import xml.etree.ElementTree as ET

import pcbnew

ROOT = Path(__file__).resolve().parent
board = pcbnew.LoadBoard(str(ROOT / "daily-mirror-p4.kicad_pcb"))
schematic = {}
netlist = ET.parse(ROOT / "output/netlist.xml")
controls = {p['ref'] for p in json.loads((ROOT / 'parts.json').read_text())}
for net in netlist.findall(".//nets/net"):
    nodes = {(n.get("ref"), n.get("pin")) for n in net.findall("node") if n.get('ref') in controls}
    if not nodes:
        continue
    schematic[net.get("name")] = nodes
physical = {}
component_count = 0
for fp in board.GetFootprints():
    if fp.GetAttributes() & pcbnew.FP_BOARD_ONLY:
        continue
    component_count += 1
    if fp.GetReference() not in controls:
        continue
    for pad in fp.Pads():
        physical.setdefault(pad.GetNetname(), set()).add((fp.GetReference(), pad.GetNumber()))
assert physical == schematic, {"schematic": schematic, "board": physical}

# All imported schematic pin numbers must exist on their assigned physical pads.
# J2's MP pads are metal mounting tabs, not ribbon contacts.
libpins = {p.get('part'): {n.get('num') for n in p.findall('pins/pin')}
           for p in netlist.findall('.//libparts/libpart')}
footprints = {f.GetReference(): f for f in board.GetFootprints()}
components = netlist.findall('.//components/comp')
assert len(components) == component_count
for c in components:
    ref = c.get('ref')
    pins = libpins[c.find('libsource').get('part')]
    pads = {p.GetNumber() for p in footprints[ref].Pads() if p.GetNumber()}
    assert pads == pins | ({'MP'} if ref == 'J2' else set()), (ref, pins, pads)
p4 = next(p for p in netlist.findall('.//libparts/libpart') if p.get('part') == 'ESP32-P4X')
assert next(p for p in p4.findall('pins/pin') if p.get('num') == '54').get('name') == 'VDD_HP_1'
for p in json.loads((ROOT / 'candidate-parts.json').read_text()):
    assert all(pad.GetNetCode() == 0 for pad in footprints[p['ref']].Pads()), p['ref']

model_paths = re.findall(r'\(model "([^"]+)"', (ROOT / 'daily-mirror-p4.kicad_pcb').read_text())
assert all(Path(p.replace('${KIPRJMOD}',str(ROOT))).exists() for p in model_paths)

# Verify the reported loose-LED pin mapping and independent limiting resistors.
for pin, color, resistor in [("1", "B", "R1"), ("2", "G", "R2"), ("4", "R", "R3")]:
    assert schematic[f"/LED_{color}_K"] == {("D1", pin), (resistor, "2")}
    assert (resistor, "1") in schematic[f"/STATUS_{color}_N"]
assert ("D1", "3") in schematic["/+3V3"]
assert ("SW1", "1") in schematic["/USER_BUTTON_N"]
assert ("SW1", "2") in schematic["/GND"]

erc = json.loads((ROOT / "output/erc.json").read_text())
drc = json.loads((ROOT / "output/drc.json").read_text())
erc_violations = [v for sheet in erc["sheets"] for v in sheet["violations"]]
assert not erc['sheets'][0]['violations'], 'Connected controls page regressed'
assert {v['type'] for v in erc_violations} <= {'pin_not_connected','power_pin_not_driven','pin_not_driven'}
assert not any(v['type']=='courtyards_overlap' for v in drc['violations']), 'Component bodies overlap'

result = {
    "design_status": "A1 imported candidate parts; only controls wired; unrouted; not for fabrication",
    "kicad_version": pcbnew.GetBuildVersion(),
    "schematic_components_matched_to_board": component_count,
    "nets_matched": len(schematic),
    "connected_controls_components": len(controls),
    "candidate_components_with_unassigned_connections": component_count-len(controls),
    "all_component_pin_numbers_match_footprints": True,
    "p4_pin54_is_current_revision_power_pin": True,
    "referenced_3d_models_resolve": len(model_paths),
    "rgb_pin_mapping_and_series_resistors": "verified",
    "button_polarity": "verified",
    "erc_violations": len(erc_violations),
    "erc_by_type": dict(collections.Counter(v['type'] for v in erc_violations)),
    "drc_placement_violations": len(drc["violations"]),
    "drc_by_type": dict(collections.Counter(v['type'] for v in drc['violations'])),
    "unrouted_connections": len(drc["unconnected_items"]),
    "copper_layers": board.GetCopperLayerCount(),
    "circuits_with_imported_parts_but_wiring_pending": ["processor", "radio", "power", "camera connector", "white light driver"],
    "camera_on_P4_validated": False,
    "mechanical_dimensions_final": False,
}
(ROOT / "output/validation.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2))
