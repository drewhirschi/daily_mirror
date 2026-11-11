"""FreeCADCmd verify.py. Reopen the saved CAD, STEP and STL files; check export
integrity, then measure the fits that matter: Pi ports against their windows,
Pi parts against the lid, camera against the lid, the ribbon's bends and gaps,
and the button's depth. Writes output/verify.json.
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
        'CameraConnector', 'CSIRibbon', 'Button']
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
gap('Ribbon to Button', ribbon, obj('Button'), 3.0)
value('Ribbon fold-back to microSD-end wall', f['fold_back_to_y'] - T, 1.0)
value('Ribbon first bend starts above the CSI socket', f['lower_run_z'] - f['bend_radius'] - (ZP + 5.5), 0.5)

# Button.
button = obj('Button')
gap('Button to Pi parts', button, pi, 1.0)
gap('Button to camera', button, Part.makeCompound([obj('CameraPCB'), obj('CameraConnector')]), 1.0)
value('Button terminals to thinned floor', lay['button_room_behind_tips'], 2.5)
value('Button hole over the M19 thread (diametral)', C['button_hole'] - C['button_thread_diameter'], 0.2)
value('Bezel covers the hole (radial)', (C['button_bezel_diameter'] - C['button_hole']) / 2, 1.0)

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
