"""FreeCADCmd verify.py. Reopen saved CAD, STEP and STL; check export integrity,
then check the real Waveshare STEP against the tray, lid, perfboard, headers,
LED/button allowances and camera, and test the camera-inside-the-lid option.
"""
from pathlib import Path
import json
import FreeCAD as App
import Part
import Mesh

HERE = Path(__file__).resolve().parent
ROOT = HERE / 'output'
C = json.loads((HERE / 'dimensions.json').read_text())
V = App.Vector
doc = App.openDocument(str(ROOT / 'DailyMirror-P4-Open.FCStd'))
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
    print('ROUNDTRIP_OK', name)

lay = json.loads((ROOT / 'layout.json').read_text())
pose = json.loads((ROOT / 'assembly-board-pose.json').read_text())
p4 = Part.Shape()
p4.read(str(HERE.parent / 'enclosure-p4' / 'reference' / 'ESP32-P4-WIFI6-3d-20260109.stp'))
place = App.Placement(V(*pose['board_translate']), App.Rotation(V(0, 0, 1), pose['board_rotate_z']))
solids = []
for s in p4.Solids:
    s = s.copy()
    s.transformShape(place.toMatrix())
    solids.append(s)
pcb_top = lay['p4_pcb_z'][1]
headers = [s for s in solids if s.BoundBox.ZMin < pcb_top - 10]   # male pin rows reaching the perfboard
others = [s for s in solids if s not in headers]
assert len(headers) == 2, len(headers)
p4bb = Part.makeCompound(solids).BoundBox

checks, notes = [], {}
def overlap(name, shape, pool, allowance=1e-4):
    bb = shape.BoundBox
    v = 0.0
    for s in pool:
        if s.BoundBox.intersect(bb):
            v += s.common(shape).Volume
    checks.append(dict(name=name, overlap_mm3=round(v, 5), passed=v < allowance))
    return v

def bbox_gap(a, b):
    dx = max(a.XMin - b.XMax, b.XMin - a.XMax, 0)
    dy = max(a.YMin - b.YMax, b.YMin - a.YMax, 0)
    dz = max(a.ZMin - b.ZMax, b.ZMin - a.ZMax, 0)
    return (dx*dx + dy*dy + dz*dz) ** .5

def gap(name, shape, pool):
    # distToShape returns 0 for some tiny STEP solids far away, so use exact
    # distance only where bounding boxes overlap and the (smaller) box gap otherwise.
    d = min(s.distToShape(shape)[0] if bbox_gap(s.BoundBox, shape.BoundBox) == 0 else
            bbox_gap(s.BoundBox, shape.BoundBox) for s in pool)
    notes[name] = round(d, 3)
    return d

obj = lambda n: doc.getObject(n).Shape
for n in ('Tray', 'CameraLid', 'Perfboard', 'LEDAllowance', 'ButtonAllowance', 'CameraPCB',
          'CameraLens', 'CameraConnector', 'CSIRibbonAllowance', 'PowerPlugAllowance'):
    overlap('P4 STEP / ' + n, obj(n), solids)
for n in ('FemaleHeaderLeft', 'FemaleHeaderRight'):
    overlap('P4 STEP (not pins) / ' + n, obj(n), others)
# Pins must actually sit over the female headers.
for h in headers:
    hb = h.BoundBox
    assert any(obj(n).BoundBox.isInside(V(hb.Center.x, hb.Center.y, lay['perfboard_top_z'] + 1))
               for n in ('FemaleHeaderLeft', 'FemaleHeaderRight')), 'P4 pins miss the headers'

rim = lay['lid_plane_z']
notes['p4_step_top_z'] = round(p4bb.ZMax, 3)
notes['p4_step_y_range'] = [round(p4bb.YMin, 3), round(p4bb.YMax, 3)]
notes['lid_plane_z'] = rim
notes['p4_headroom_below_lid_plane'] = round(rim - p4bb.ZMax, 3)
notes['led_headroom_below_lid_plane'] = round(rim - lay['led_top_z'], 3)
gap('min_gap_p4_to_tray', obj('Tray'), solids)
gap('min_gap_p4_to_lid', obj('CameraLid'), solids)
gap('min_gap_p4_to_led', obj('LEDAllowance'), solids)
assert p4bb.ZMax < rim and lay['led_top_z'] < rim, 'something pokes above the lid plane'

# Option not taken: camera on the lid's INSIDE face, lens up through a hole.
# PCB pressed against the lid underside, connector + back parts hanging below.
cx, (cy0, cy1) = lay['camera_cx'], lay['camera_board_y']
cw, ct, ch = C['camera_board_width'], C['camera_board_thickness'], C['camera_connector_height']
in_pcb = Part.makeBox(cw, cy1 - cy0, ct, V(cx - cw / 2, cy0, rim - ct))
in_conn = Part.makeBox(21, C['camera_connector_depth'], ch, V(cx - 10.5, cy0, rim - ct - ch))
inside = dict(camera_pcb_underside_z=round(rim - ct, 3), connector_underside_z=round(rim - ct - ch, 3))
inside['overlap_pcb_mm3'] = round(sum(s.common(in_pcb).Volume for s in solids if s.BoundBox.intersect(in_pcb.BoundBox)), 3)
inside['overlap_connector_mm3'] = round(sum(s.common(in_conn).Volume for s in solids if s.BoundBox.intersect(in_conn.BoundBox)), 3)
under = [s for s in solids if s.BoundBox.YMax > cy0 and s.BoundBox.YMin < cy1
         and s.BoundBox.XMax > cx - cw / 2 and s.BoundBox.XMin < cx + cw / 2]
inside['p4_top_under_camera_z'] = round(max(s.BoundBox.ZMax for s in under), 3)
inside['gap_connector_to_p4'] = round(inside['connector_underside_z'] - inside['p4_top_under_camera_z'], 3)
inside['csi_connector_top_z'] = lay['csi_connector'][4]
# A 15-pin FPC needs ~3 mm to turn from the camera connector down into the vertical CSI socket.
inside['lid_plane_needed_z'] = round(inside['p4_top_under_camera_z'] + 0.5 + ch + ct + 3.0, 2)
inside['total_height_needed'] = round(inside['lid_plane_needed_z'] + C['lid_thickness'], 2)
inside['collides'] = inside['overlap_pcb_mm3'] + inside['overlap_connector_mm3'] > 1e-3 or inside['gap_connector_to_p4'] < 1.0
notes['inside_mount_option'] = inside

failed = [c for c in checks if not c['passed']]
(ROOT / 'verify.json').write_text(json.dumps(dict(checks=checks, notes=notes, failed=len(failed)), indent=2) + '\n')
for c in checks:
    print('CHECK', 'ok  ' if c['passed'] else 'FAIL', c['name'], c['overlap_mm3'])
print('NOTES', json.dumps(notes, indent=1))
assert not failed, failed
print('VERIFY_OK', len(checks), 'STEP interference checks')
App.closeDocument(doc.Name)
