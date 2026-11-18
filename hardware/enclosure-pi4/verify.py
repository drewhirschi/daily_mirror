"""FreeCADCmd verify.py. Reopen the saved CAD, STEP and STL files; check export
integrity, then measure the fits that matter: Pi ports against their windows,
Pi parts against the lid, camera against the lid, the ribbon's bends and gaps,
and the switch, cap and LED stack under the lid. Writes output/verify.json.
"""
from pathlib import Path
import json, math
import FreeCAD as App
import Part
import Mesh

HERE = Path(__file__).resolve().parent
ROOT = HERE / 'output'
C = json.loads((HERE / 'dimensions.json').read_text())
V = App.Vector
doc = App.openDocument(str(ROOT / 'DailyMirror-Pi4.FCStd'))
for rec in json.loads((ROOT / 'manifest.json').read_text()):
    name = rec['name']
    shape = doc.getObject(name).Shape
    step = Part.read(str(ROOT / (name + '.step')))
    mesh = Mesh.Mesh(str(ROOT / (name + '.stl')))
    assert shape.isValid() and step.isValid() and mesh.isSolid(), name
    assert len(step.Solids) == 1, name
    assert abs(shape.Volume - step.Volume) < .01, name
    assert abs(abs(mesh.Volume) - shape.Volume) / shape.Volume < .005, name
    assert mesh.BoundBox.ZMin >= -.0001, name
    assert all(abs(a - b) < .01 for a, b in zip(rec['print_size_mm'],
               (mesh.BoundBox.XLength, mesh.BoundBox.YLength, mesh.BoundBox.ZLength))), name
    assert max(mesh.BoundBox.XLength, mesh.BoundBox.YLength) < 250, (name, 'does not fit the A1 bed')
    print('ROUNDTRIP_OK', name)

lay = json.loads((ROOT / 'layout.json').read_text())
obj = lambda n: doc.getObject(n).Shape
tray, lid = obj('Tray'), obj('Lid')
W, L, H = lay['outer_mm']
T, RIM = C['wall'], lay['rim_z']
X0, Y0, ZP = lay['pi_origin']
checks = []

def overlap(name, a, b):
    v = a.common(b).Volume
    checks.append(dict(name=name, kind='interference', overlap_mm3=round(v, 5), passed=v < 1e-4))

def gap(name, a, b, need):
    d = a.distToShape(b)[0]
    checks.append(dict(name=name, kind='gap', gap_mm=round(d, 3), required_mm=need, passed=d >= need - 1e-6))

def value(name, got, need, kind='margin'):
    checks.append(dict(name=name, kind=kind, value_mm=round(got, 3), required_mm=need, passed=got >= need - 1e-6))

refs = ['PiPCB', 'PiPorts', 'PiHeaders', 'PiChips', 'PiSDCard', 'PiUndersideAllowance', 'GPIOJumperAllowance',
        'PowerPlugAllowance', 'HDMIPlugAllowance', 'CameraPCB', 'CameraLens', 'CameraFrontParts',
        'CameraConnector', 'CSIRibbon', 'Switch', 'StatusLED', 'ButtonCap', 'ButtonCarrier',
        'ControlPCB', 'J1Header', 'DupontAllowance', 'ControlResistors']
for n in refs:
    overlap(n + ' / Tray', obj(n), tray)
    overlap(n + ' / Lid', obj(n), lid)
overlap('Tray / Lid', tray, lid)

# The Pi drops straight in: nothing on it reaches a wall.
pi = Part.makeCompound([obj(n) for n in ('PiPCB', 'PiPorts', 'PiHeaders', 'PiChips')])
bb = pi.BoundBox
value('Pi drop-in: port side to wall', (W - T) - bb.XMax, 0.3)
value('Pi drop-in: GPIO side to wall', bb.XMin - T, 0.5)
value('Pi drop-in: microSD end to wall', bb.YMin - T, 0.5)
gap('Pi parts to Lid (lip included)', pi, lid, 1.0)
value('USB stack top to lid underside', RIM - bb.ZMax, 2.0)
gap('GPIO jumpers to Lid', obj('GPIOJumperAllowance'), lid, 1.0)
gap('Pi underside (pin tails) to Tray floor and posts', obj('PiUndersideAllowance'), tray, 0.0)

# Ports against their windows: margin between each plug envelope and the tray.
gap('USB-C plug (13 x 7) to window edge', obj('PowerPlugAllowance'), tray, 0.2)
if C['hdmi_windows']:
    gap('micro-HDMI plugs (11 x 6.5) to window edge', obj('HDMIPlugAllowance'), tray, 0.2)
# Window centres against the connector centres, straight from the Pi drawing numbers.
ux, uy, uz = lay['usb_c_centre']
value('USB-C window centred on the port (Y error)', 0.05 - abs(uy - (Y0 + 11.2)), 0, 'alignment')
value('USB-C window centred on the port (Z error)', 0.05 - abs(uz - (ZP + 1.6)), 0, 'alignment')
for i, (hx, hy, hz) in enumerate(lay['hdmi_centres']):
    value(f'micro-HDMI {i} window centred on the port', 0.05 - abs(hy - (Y0 + (26.0, 39.5)[i])), 0, 'alignment')
card = obj('PiSDCard').Solids[1]
gap('microSD card to slot edge', card, tray, 0.3)
value('microSD card reaches into the slot', T - card.BoundBox.YMin, 0.5)

# Camera: square to the lid by construction (four equal posts); lens in the bore.
lens = obj('CameraLens')
gap('Lens block/barrel to Lid (bore and collar)', lens, lid, 0.25)
gap('Camera front parts to Lid collar', obj('CameraFrontParts'), lid, 0.5)
lx, ly = lay['lens_centre']
lc = lens.Solids[0].BoundBox.Center
value('Lens centred in the bore', 0.02 - math.hypot(lc.x - lx, lc.y - ly), 0, 'alignment')
value('Lens barrel recessed below the lid face', lay['lens_recess_below_face'], 0.5)
value('Collar overlaps the lens block', C['camera_collar_height'] - C['camera_lens_block_gap'], 1.0)
for i, (x, y) in enumerate(lay['camera_holes']):
    post = Part.makeCylinder(.3, 1, V(x, y, lay['camera_pcb_z'][1]))
    assert lid.common(post).Volume < 1e-6 and abs(lid.distToShape(post)[0] - (C['camera_pilot']/2 - .3)) < .05, ('camera post misses hole', i)
gap('Camera connector to Pi parts', obj('CameraConnector'), pi, 1.0)
gap('Camera PCB to Tray walls', obj('CameraPCB'), tray, 1.0)

# Ribbon.
ribbon, f = obj('CSIRibbon'), lay['fpc']
value('Ribbon bend radius', f['bend_radius'], 3.0, 'bend')
value('Ribbon length matches the cable (error)', 0.5 - abs(f['length'] - C['fpc_length']), 0, 'alignment')
heads = obj('PiHeaders').Solids
low = Part.makeCompound([obj('PiPCB'), obj('PiChips')] + obj('PiPorts').Solids[:4] + [heads[0], heads[2], heads[3]])
free = ribbon.common(Part.makeBox(200, 400, 100, V(-50, -50, ZP + 5.5 + .05)))      # the part outside the CSI socket
gap('Ribbon (outside the socket) to Pi board, low parts, GPIO, DSI', free, low, 2.0)
gap('Ribbon to USB/Ethernet stacks', ribbon, Part.makeCompound(obj('PiPorts').Solids[4:]), 1.0)
gap('Ribbon to GPIO jumpers', ribbon, obj('GPIOJumperAllowance'), 3.0)
gap('Ribbon to Lid', ribbon, lid, 1.0)
gap('Ribbon to Tray', ribbon, tray, 1.0)
gap('Ribbon to switch, carrier and LED', ribbon, Part.makeCompound([obj('Switch'), obj('ButtonCarrier'), obj('StatusLED')]), 3.0)
value('Ribbon fold-back to microSD-end wall', f['fold_back_to_y'] - T, 1.0)
value('Ribbon first bend starts above the CSI socket', f['lower_run_z'] - f['bend_radius'] - (ZP + 5.5), 0.5)

# Button: tactile switch on its carrier, printed cap through the lid, LED beside it.
switch, cap, carrier, led = obj('Switch'), obj('ButtonCap'), obj('ButtonCarrier'), obj('StatusLED')
overlap('ButtonCap / Switch', cap, switch)
overlap('ButtonCarrier / Switch', carrier, switch)
overlap('StatusLED / ButtonCarrier', led, carrier)
stack = Part.makeCompound([switch, carrier, led])
value('Switch depth behind the lid underside, to the terminal tips (15 max)', 15.0 - lay['switch_depth_behind_lid'], 0)
value('LED depth behind the lid underside, lead tails under the PCB (15 max)', 15.0 - lay['led_depth_behind_lid'], 0)
gap('Switch, carrier and LED to Pi parts', stack, pi, 1.0)
gap('Switch, carrier and LED to camera', stack, Part.makeCompound([obj('CameraPCB'), obj('CameraConnector')]), 1.0)
gap('Switch and carrier to Tray', Part.makeCompound([switch, carrier]), tray, 1.0)
gap('Cap stem to lid hole (radial)', cap, lid, 0.25)
value('Cap flange overlaps the lid hole (radial)', (C['cap_flange_diameter'] - C['cap_hole']) / 2, 1.2)
value('Cap stays proud of the lid when pressed', C['cap_proud'] - C['switch_travel'], 0.5)
value('Cap free play at rest (flange to lid)', C['cap_rest_gap'], 0.2)
body_top = lay['switch_seat_z'] + C['switch_plunger_base_height']
value('Cap flange to the switch (4.3 mm level) beyond the pretravel', cap.BoundBox.ZMin - body_top - C['switch_travel'], 1.0)
value('Cap socket sits on the plunger (error)', 0.02 - abs(switch.BoundBox.ZMax - (cap.BoundBox.ZMin + C['cap_socket_depth'])), 0, 'alignment')
value('Cap socket over the plunger (radial)', C['cap_socket_clearance'], 0.2)
for i, (x, y) in enumerate(lay['switch_posts']):
    probe = Part.makeCylinder(.3, 1, V(x, y, lay['switch_seat_z']))
    assert lid.common(probe).Volume < 1e-6 and abs(lid.distToShape(probe)[0] - (C['camera_pilot']/2 - .3)) < .05, ('switch post misses', i)
    hole = Part.makeCylinder(.3, 5, V(x, y, lay['switch_seat_z'] - 3))
    assert carrier.common(hole).Volume < 1e-6, ('carrier hole misses the post', i)
value('Switch body between its posts (per side)', C['switch_post_spacing']/2 - C['camera_post_diameter']/2 - C['switch_body']/2, 0.4)
value('Carrier slot over the terminal (smallest side margin)', min(C['carrier_slot'][0] - C['switch_terminal_section'][1],
      C['carrier_slot'][1] - C['switch_terminal_section'][0]) / 2, 0.3)
value('Terminal tails below the carrier, for soldering', C['switch_terminal_length'] - C['carrier_thickness'], 1.2)
value('LED hole over the LED body (diametral)', C['led_hole'] - C['led_diameter'], 0.1)
value('Carrier hole over the locating boss (diametral)', C['carrier_boss_hole'] - C['switch_boss_diameter'], 0.3)

# Control PCB (replaces the ButtonCarrier) with the 5 mm RGB LED, J1 and R1-R3.
cp, j1, du, res = obj('ControlPCB'), obj('J1Header'), obj('DupontAllowance'), obj('ControlResistors')
pcb = lay['control_pcb']
value('Control PCB thickness equals the carrier (error)', 0.01 - abs((pcb['z'][1] - pcb['z'][0]) - C['carrier_thickness']), 0, 'alignment')
value('Control PCB top face is the switch seating plane (error)', 0.01 - abs(pcb['z'][1] - lay['switch_seat_z']), 0, 'alignment')
value('Control PCB and ButtonCarrier underside at the same height (error)', 0.01 - abs(cp.BoundBox.ZMin - carrier.BoundBox.ZMin), 0, 'alignment')
for i, (x, y) in enumerate(lay['switch_posts'] + lay['led_posts']):
    probe = Part.makeCylinder(.3, 1, V(x, y, lay['switch_seat_z']))
    assert lid.common(probe).Volume < 1e-6 and abs(lid.distToShape(probe)[0] - (C['camera_pilot']/2 - .3)) < .05, ('post misses', i)
    hole = Part.makeCylinder(.3, 5, V(x, y, lay['switch_seat_z'] - 3))
    assert cp.common(hole).Volume < 1e-6, ('PCB hole misses the post', i)
value('LED dome tip flush with the lid face (error)', 0.02 - abs(led.BoundBox.ZMax - H), 0, 'alignment')
value('LED flange above the PCB (soldering standoff)', lay['led_standoff_above_pcb'], 0.5)
gap('LED to the lid hole and collar (radial)', led, lid, 0.2)
value('LED flange below the collar', lay['led_collar_z'][0] - lay['led_flange_z'][1], 0.3)
cap_flange = C['cap_flange_diameter'] / 2
value('LED collar to the button cap flange (plan)', C['led_x'] - C['led_collar_diameter']/2 - (lay['button_centre'][0] + cap_flange), 1.0)
gap('LED collar to button cap', cap, lid.common(Part.makeCylinder(C['led_collar_diameter']/2 + .05, RIM - lay['led_collar_z'][0] - .01,
    V(C['led_x'], lay['led_centre'][1], lay['led_collar_z'][0]))), 1.0)
led_posts = Part.makeCompound([lid.common(Part.makeCylinder(C['camera_post_diameter']/2 + .05, RIM - lay['switch_seat_z'] + .01,
                                V(x, y, lay['switch_seat_z']))) for x, y in lay['led_posts']])
cam_all = Part.makeCompound([obj(n) for n in ('CameraPCB', 'CameraLens', 'CameraFrontParts', 'CameraConnector')])
cam_posts = Part.makeCompound([lid.common(Part.makeCylinder(C['camera_post_diameter']/2 + .05, lay['camera_post_height'] + .01,
                               V(x, y, lay['camera_pcb_z'][1]))) for x, y in lay['camera_holes']])
gap('New LED posts to camera board and parts', led_posts, cam_all, 1.0)
gap('New LED posts to camera posts', led_posts, cam_posts, 1.0)
gap('New LED posts to ribbon', led_posts, ribbon, 1.0)
gap('New LED posts to button cap', led_posts, cap, 1.0)
gap('New LED posts to switch', led_posts, switch, 1.0)
gap('New LED posts to LED', led_posts, led, 1.0)
gap('Control PCB to camera board, connector and lens parts', cp, cam_all, 1.0)
gap('Control PCB to camera posts', cp, cam_posts, 1.0)
gap('Control PCB to ribbon', cp, ribbon, 1.0)
gap('Control PCB to Tray walls', cp, tray, 1.0)
gap('Control PCB to USB/Ethernet stacks', cp, Part.makeCompound(obj('PiPorts').Solids[4:]), 1.0)
gap('Control PCB to lid features other than its four posts', cp, lid.common(Part.makeBox(W + 20, L + 20, RIM - lay['switch_seat_z'] - .01, V(-10, -10, lay['switch_seat_z'] + .01))).cut(
    Part.makeCompound([Part.makeCylinder(C['camera_post_diameter']/2 + .05, 40, V(x, y, 0)) for x, y in lay['switch_posts'] + lay['led_posts']])), 1.0)
parts_back = Part.makeCompound([j1, res])
gap('J1 and R1-R3 to Pi parts', parts_back, pi, 1.0)
gap('J1 and R1-R3 to Tray', parts_back, tray, 1.0)
gap('Dupont housing + wire bend to Tray (floor, walls)', du, tray, 1.0)
gap('Dupont housing + wire bend to Pi parts (USB/Ethernet stacks)', du, pi, 1.0)
gap('Dupont housing + wire bend to ribbon', du, ribbon, 3.0)
gap('Dupont housing + wire bend to camera', du, cam_all, 1.0)
gap('Dupont housing to R1-R3', du, res, 1.0)
gap('R1-R3 (lead tails on the front) to switch', res, switch, 1.0)
gap('Control PCB parts to ribbon', Part.makeCompound([j1, res, led]), ribbon, 3.0)
value('Dupont housing bottom above the tray floor', pcb['dupont_housing_bottom_z'] - T, 4.0)

failed = [c for c in checks if not c['passed']]
notes = dict(outer_mm=lay['outer_mm'], headroom_over_pcb=lay['headroom_over_pcb'], fpc=f,
             fpc_lengths_that_fit_mm=[f['min_length'], f['max_length']])
(ROOT / 'verify.json').write_text(json.dumps(dict(status='CAD checks only; physical fit untested', checks=checks,
                                                   notes=notes, failed=len(failed)), indent=2) + '\n')
for c in checks:
    print('CHECK', 'ok  ' if c['passed'] else 'FAIL', c['name'], c.get('gap_mm', c.get('value_mm', c.get('overlap_mm3'))))
assert not failed, failed
print('VERIFY_OK', len(checks), 'checks')
App.closeDocument(doc.Name)
