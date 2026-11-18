"""FreeCADCmd build.py. All dimensions mm; dimensions.json is the single source.

Open-top tray for the Waveshare ESP32-P4-WIFI6 plugged into a 50 x 70 perfboard,
with a short screwed-on CameraLid over the top end of the board carrying the
IMX519 on its outer face. Same axes as enclosure-p4: X across, Y along the
board (USB-C end at Y=0, lid end at Y=L), Z out from the flat back (Z=0).
"""
import json
from pathlib import Path
import FreeCAD as App
import Part
import MeshPart

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
OUT.mkdir(exist_ok=True)
C = json.loads((ROOT / 'dimensions.json').read_text())
STEP = ROOT.parent / 'enclosure-p4' / 'reference' / 'ESP32-P4-WIFI6-3d-20260109.stp'
V = App.Vector
T, CL, LT = C['wall'], C['perfboard_clearance'], C['lid_thickness']
PBW, PBH, PBT = C['perfboard_width'], C['perfboard_height'], C['perfboard_thickness']

# P4 facts from Waveshare's STEP (board frame: module end Y=0, USB-C end Y=71.05,
# PCB top Z=0). Checked against the real STEP in verify.py.
P4_W, P4_L, P4_T = 21.0, 71.05, 1.6
P4_TOP_PARTS = 4.7            # tallest top-side part (vertical 2-pin connector at Y 19.4-23)
P4_USB_OVERHANG = 1.35        # USB-C shell past the board end
P4_USB_Z = 1.17               # USB-C centre above PCB top (mid-mount)
P4_CSI = (2.24, 18.76, 33.27, 36.77, 4.1)   # x0, x1, y0, y1, height of the vertical CSI FPC connector
P4_HEADER_X = (1.61, 19.39)   # pin-row centres
P4_HEADER_Y = (17.0, 67.8)    # 1x20 rows

DOC = App.newDocument('DailyMirrorP4Open')
PRINT = DOC.addObject('App::DocumentObjectGroup', 'PrintableParts')
REF = DOC.addObject('App::DocumentObjectGroup', 'HardwareReferences')
FIT = DOC.addObject('App::DocumentObjectGroup', 'FitCoupons')
parts, refs, coupons = [], [], []

def box(x, y, z, w, h, d):
    return Part.makeBox(w, h, d, V(x, y, z))

def cylinder(x, y, z, diameter, height, axis=(0, 0, 1)):
    return Part.makeCylinder(diameter / 2, height, V(x, y, z), V(*axis))

def union(shapes):
    return shapes[0].multiFuse(shapes[1:]).removeSplitter() if len(shapes) > 1 else shapes[0]

def subtract(shape, cutters):
    return shape.cut(Part.makeCompound(cutters)).removeSplitter()

def roundbox(x, y, z, w, h, d, r):
    return union([box(x+r, y, z, w-2*r, h, d), box(x, y+r, z, w, h-2*r, d)] +
                 [cylinder(x+a, y+b, z, 2*r, d) for a in (r, w-r) for b in (r, h-r)])

def add(name, shape, color, group=PRINT, note=''):
    shape = shape.removeSplitter()
    assert not shape.isNull() and shape.isValid(), name
    assert len(shape.Solids) == 1, (name, len(shape.Solids))
    obj = DOC.addObject('Part::Feature', name)
    obj.Shape = shape
    obj.addProperty('App::PropertyString', 'AssemblyNote')
    obj.AssemblyNote = note
    group.addObject(obj)
    record = dict(name=name, obj=obj, color=color, note=note)
    (parts if group == PRINT else refs if group == REF else coupons).append(record)
    return shape

# LAYOUT ---------------------------------------------------------------------
H = C['total_height']
RIM = H - LT                                   # tray rim = lid underside = "lid plane"
W = 2*T + 2*CL + PBW
L = T + C['usb_end_clearance'] + PBH + CL + T
CX = W / 2
pb_x, pb_y = T + CL, T + C['usb_end_clearance']
pb_z = T + C['perfboard_post_height']
pb_top = pb_z + PBT
if C['perfboard_holes']:
    pb_holes = [(pb_x+x, pb_y+y) for x, y in C['perfboard_holes']]
else:
    ix, iy = C['perfboard_hole_inset_x'], C['perfboard_hole_inset_y']
    pb_holes = [(pb_x+x, pb_y+y) for x in (ix, PBW-ix) for y in (iy, PBH-iy)]

p4_x0 = pb_x + C['p4_offset_x']
p4_x1 = p4_x0 + P4_W
p4_cx = (p4_x0 + p4_x1) / 2
p4_y1 = pb_y + PBH - C['p4_end_inset']         # module (antenna) end, under the lid
p4_y0 = p4_y1 - P4_L                           # USB-C end, toward Y=0
p4_z = pb_top + C['female_header_height'] + C['male_header_spacer']   # PCB underside
p4_top = p4_z + P4_T
p4_parts_top = p4_top + P4_TOP_PARTS
usb_z = p4_top + P4_USB_Z
usb_mouth_y = p4_y0 - P4_USB_OVERHANG
# Board frame -> assembly: rotate 180 about Z, then translate.
csi_x0, csi_x1 = p4_x1 - P4_CSI[1], p4_x1 - P4_CSI[0]
csi_y0, csi_y1 = p4_y1 - P4_CSI[3], p4_y1 - P4_CSI[2]
csi_top = p4_top + P4_CSI[4]

# Camera on the lid's OUTER face, lens out, top edge toward the lid end,
# 15-pin connector (on the camera's back) on the edge facing the open tray.
CBW, CBL = C['camera_board_width'], C['camera_board_length']
cam_cx = p4_cx + C['camera_x_offset']
cam_top = L - C['camera_end_margin']
cam_bottom = cam_top - CBL
cam_z = RIM + LT + C['camera_standoff_height']   # camera PCB underside
cam_lens_y = cam_top - C['camera_lens_from_top_edge']
hs = C['camera_hole_from_top_edge']
cam_holes = [(cam_cx+x, cam_top-y) for x in (-C['camera_hole_spacing_x']/2, C['camera_hole_spacing_x']/2)
             for y in (hs, hs+C['camera_hole_spacing_y'])]
lid_y0 = cam_bottom + C['camera_overhang']      # lid's inner edge; the ribbon drops past it
LID_LEN = L - lid_y0
ear_d, ear_off = C['lid_ear_diameter'], C['lid_ear_offset']
ear_ys = (L - C['lid_ear_inset_from_end'], lid_y0 + C['lid_ear_inset_from_edge'])
ears = [(x, y) for x in (-ear_off, W + ear_off) for y in ear_ys]
led_x, led_y = pb_x + C['led_x'], pb_y + C['led_y']
btn_x, btn_y = pb_x + C['button_x'], pb_y + C['button_y']

assert p4_parts_top < RIM - 1.0, ('P4 parts above the lid plane', p4_parts_top, RIM)
assert pb_top + C['led_height'] < RIM, ('LED above the lid plane', pb_top + C['led_height'], RIM)
assert pb_z - C['solder_side_allowance'] > T, 'solder side hits the floor'
assert lid_y0 > max(led_y, btn_y + C['button_size']/2), 'lid covers the LED or button'
assert C['camera_standoff_height'] >= C['camera_connector_height'] + 0.5, 'camera connector hits the lid'
assert lid_y0 - csi_y1 > 0, 'CSI connector must be in the open part of the tray'

# TRAY: flat back on the bed, four perfboard posts, two screw ears per side at the lid end.
outline = roundbox(0, 0, 0, W, L, RIM, C['corner_radius'])
tray = outline.cut(roundbox(T, T, T, W-2*T, L-2*T, RIM, max(C['corner_radius']-T, .5)))
tray = union([tray] +
    [cylinder(x, y, T-.01, C['perfboard_post_diameter'], C['perfboard_post_height']+.01) for x, y in pb_holes] +
    [cylinder(x, y, 0, ear_d, RIM) for x, y in ears])
cuts = [cylinder(x, y, RIM-C['lid_screw_depth'], C['case_pilot'], C['lid_screw_depth']+1) for x, y in ears]
cuts += [cylinder(x, y, T+.5, C['perfboard_pilot'], C['perfboard_post_height']) for x, y in pb_holes]
# Power entry: U-notch in the USB-end wall, open to the top so the plug drops in.
nw = C['power_notch_width']
cuts += [box(p4_cx-nw/2, -1, usb_z-C['power_notch_below_usb'], nw, T+2, RIM)]
tray = add('Tray', subtract(tray, cuts), (.28, .34, .38),
    note='Print back face on the bed, no supports. Perfboard: four M2 x 6 thread-forming screws. '
         'Lid: four M3 x 10 thread-forming screws into the ears.')

# CAMERA LID: plate over the top end, camera posts on the outside face.
r = C['corner_radius']
lid = roundbox(0, lid_y0-r, RIM, W, LID_LEN+r, LT, r).cut(box(-1, lid_y0-r-1, RIM-1, W+2, r+1, LT+2))
lid = union([lid] + [cylinder(x, y, RIM, ear_d, LT) for x, y in ears] +
            [cylinder(x, y, RIM+LT-.01, C['camera_post_diameter'], C['camera_standoff_height']+.01)
             for x, y in cam_holes])
pilot_depth = C['camera_standoff_height'] + LT - .6
cuts = [cylinder(x, y, RIM-1, C['lid_screw_clearance'], LT+2) for x, y in ears]
cuts += [cylinder(x, y, cam_z-pilot_depth, C['camera_pilot'], pilot_depth+.1) for x, y in cam_holes]
lid = add('CameraLid', subtract(lid, cuts), (.91, .90, .85),
    note='Print underside on the bed, posts up, no supports. Camera: four M2 x 6 thread-forming screws.')

# REFERENCES: envelopes for checks and the preview (the real P4 STEP is checked in verify.py).
perf = subtract(box(pb_x, pb_y, pb_z, PBW, PBH, PBT), [cylinder(x, y, pb_z-1, 2.0, 4) for x, y in pb_holes])
add('Perfboard', perf, (.80, .62, .32), REF, note='50 x 70 perfboard; hole positions are a guess - measure yours.')
add('SolderSide', box(pb_x, pb_y, pb_z-C['solder_side_allowance'], PBW, PBH, C['solder_side_allowance']),
    (.55, .45, .25), REF, note='Solder joints and wire under the perfboard.')
for side, hx in (('Left', p4_x1-P4_HEADER_X[1]), ('Right', p4_x1-P4_HEADER_X[0])):
    add('FemaleHeader'+side, box(hx-1.27, p4_y1-P4_HEADER_Y[1], pb_top, 2.54, P4_HEADER_Y[1]-P4_HEADER_Y[0],
        C['female_header_height']), (.10, .10, .10), REF, note='1x20 female header on the perfboard.')
add('P4Board', box(p4_x0, p4_y0, p4_z, P4_W, P4_L, P4_T), (.15, .48, .32), REF,
    note='Waveshare ESP32-P4-WIFI6 PCB, USB-C toward Y=0, components up.')
add('P4Components', box(p4_x0, p4_y0, p4_top, P4_W, P4_L, P4_TOP_PARTS), (.17, .50, .35), REF,
    note='Envelope of the top-side parts (4.7 mm max).')
add('PowerPlugAllowance', box(p4_cx-6.5, -25, usb_z-3.5, 13, usb_mouth_y-.2+25, 7), (.23, .29, .32), REF,
    note='Straight USB-C plug, overmold up to 13 x 7 mm.')
fw = C['fpc_width'] + 2
add('CSIRibbonRiser', box(csi_x0, csi_y0, csi_top, csi_x1-csi_x0, csi_y1-csi_y0, p4_parts_top+1.5-csi_top),
    (.85, .75, .35), REF, note='Ribbon leaving the vertical CSI connector.')
add('CSIRibbonAllowance', box(cam_cx-fw/2, csi_y0, p4_parts_top+1.0, fw, lid_y0-.5-csi_y0, cam_z-p4_parts_top-1.0),
    (.85, .75, .35), REF, note='16 mm FPC from the P4 up past the lid edge to the camera connector.')
camera = subtract(box(cam_cx-CBW/2, cam_bottom, cam_z, CBW, CBL, C['camera_board_thickness']),
                  [cylinder(x, y, cam_z-1, 2.2, 4) for x, y in cam_holes])
add('CameraPCB', camera, (.10, .35, .20), REF, note='Arducam IMX519 in the Pi Camera v1.3 format.')
lb = C['camera_lens_block']
cam_face = cam_z + C['camera_board_thickness']
add('CameraLens', box(cam_cx-lb/2, cam_lens_y-lb/2, cam_face, lb, lb, C['camera_lens_height']),
    (.12, .12, .12), REF, note='Autofocus lens block, facing out.')
add('CameraConnector', box(cam_cx-10.5, cam_bottom, cam_z-C['camera_connector_height'], 21,
    C['camera_connector_depth'], C['camera_connector_height']), (.85, .85, .80), REF,
    note='15-pin FPC connector on the camera back, open-tray edge.')
add('LEDAllowance', cylinder(led_x, led_y, pb_top, C['led_diameter'], C['led_height']), (.82, .87, .65), REF,
    note='RGB LED on the perfboard; placeholder position and height.')
bs = C['button_size']
add('ButtonAllowance', box(btn_x-bs/2, btn_y-bs/2, pb_top, bs, bs, C['button_height']), (.34, .41, .47), REF,
    note='Push button on the perfboard; placeholder position and height.')

# COUPON: just the post pattern on a thin frame.
plate = box(pb_x-3, pb_y-3, 0, PBW+6, PBH+6, 2)
plate = plate.cut(box(pb_x+8, pb_y+8, -1, PBW-16, PBH-16, 4))
plate = union([plate] + [cylinder(x, y, 1.99, C['perfboard_post_diameter'], C['perfboard_post_height']-1)
                         for x, y in pb_holes])
plate = subtract(plate, [cylinder(x, y, 1, C['perfboard_pilot'], C['perfboard_post_height']+1) for x, y in pb_holes])
add('PerfboardFitCoupon', plate, (.28, .34, .38), FIT,
    note='Same post pattern as the Tray. Screw the perfboard on to check the hole positions.')

DOC.recompute()

def print_shape(record):
    s = record['obj'].Shape.copy()
    b = s.BoundBox
    s.translate(V(-b.XMin, -b.YMin, -b.ZMin))
    return s

def mesh_record(name, kind, color, note, shape, deflection=.12):
    mesh = MeshPart.meshFromShape(Shape=shape, LinearDeflection=deflection, AngularDeflection=.3, Relative=False)
    vertices, faces = mesh.Topology
    return dict(name=name, kind=kind, color=color, note=note,
                vertices=[[round(p.x, 3), round(p.y, 3), round(p.z, 3)] for p in vertices],
                faces=[list(f) for f in faces])

scene, manifest = [], []
for rec in parts + refs + coupons:
    s = rec['obj'].Shape
    kind = 'part' if rec in parts else 'reference' if rec in refs else 'coupon'
    scene.append(mesh_record(rec['name'], kind, rec['color'], rec['note'], s))
    if kind == 'reference':
        continue
    ps = print_shape(rec)
    pm = MeshPart.meshFromShape(Shape=ps, LinearDeflection=.08, AngularDeflection=.16, Relative=False)
    assert pm.isSolid(), rec['name']
    pm.write(str(OUT/(rec['name']+'.stl')))
    s.exportStep(str(OUT/(rec['name']+'.step')))
    b = ps.BoundBox
    manifest.append(dict(name=rec['name'], kind=kind, solid_count=len(s.Solids), valid=s.isValid(),
                         closed_mesh=pm.isSolid(), volume_mm3=round(s.Volume, 3),
                         print_size_mm=[round(v, 3) for v in (b.XLength, b.YLength, b.ZLength)], note=rec['note']))

# The real Waveshare board, placed where the headers put it.
pose = dict(board_translate=[p4_x1, p4_y1, p4_top], board_rotate_z=180)
p4 = Part.Shape()
p4.read(str(STEP))
p4.Placement = App.Placement(V(*pose['board_translate']), App.Rotation(V(0, 0, 1), 180))
scene.append(mesh_record('P4STEP', 'reference', (.12, .40, .26), 'Waveshare STEP', p4, .25))

checks = []
def clearance(name, a, b, allowance=1e-5):
    v = a.common(b).Volume
    checks.append(dict(name=name, overlap_mm3=round(v, 8), passed=v < allowance))
    assert v < allowance, (name, v)

named = {r['name']: r['obj'].Shape for r in refs}
clearance('Tray / CameraLid', tray, lid)
for n in named:
    if n != 'SolderSide':   # the solder allowance surrounds the posts by design
        clearance(n+' / Tray', named[n], tray)
    clearance(n+' / CameraLid', named[n], lid)
for a, b in (('P4Components', 'LEDAllowance'), ('P4Components', 'ButtonAllowance'),
             ('P4Board', 'LEDAllowance'), ('P4Board', 'ButtonAllowance'),
             ('LEDAllowance', 'ButtonAllowance'), ('CSIRibbonAllowance', 'P4Components'),
             ('CSIRibbonAllowance', 'LEDAllowance'), ('PowerPlugAllowance', 'Perfboard'),
             ('FemaleHeaderLeft', 'LEDAllowance'), ('FemaleHeaderRight', 'LEDAllowance'),
             ('FemaleHeaderRight', 'ButtonAllowance')):
    clearance(a+' / '+b, named[a], named[b])

layout = dict(outer_mm=[W, L, H], outer_with_ears_mm=[round(W+2*(ear_off+ear_d/2), 2), L, H],
              lid_plane_z=RIM, lid_length=round(LID_LEN, 3), lid_covers_board_mm=round(pb_y+PBH-lid_y0, 3),
              lid_inner_edge_y=round(lid_y0, 3), perfboard_origin=[pb_x, pb_y, pb_z], perfboard_top_z=pb_top,
              perfboard_holes=[[round(x-pb_x, 3), round(y-pb_y, 3)] for x, y in pb_holes],
              p4_pcb_z=[p4_z, p4_top], p4_parts_top_z=p4_parts_top, headroom_over_p4=round(RIM-p4_parts_top, 3),
              led_top_z=pb_top+C['led_height'], max_led_height_above_perfboard=round(RIM-pb_top, 3),
              usb_centre=[p4_cx, usb_mouth_y, usb_z], csi_connector=[csi_x0, csi_x1, csi_y0, csi_y1, csi_top],
              camera_pcb_z=cam_z, camera_lens_top_z=cam_face+C['camera_lens_height'],
              camera_board_y=[cam_bottom, cam_top], camera_cx=cam_cx, ears=ears, rim_z=RIM)
(OUT/'layout.json').write_text(json.dumps(layout, indent=2)+'\n')
(OUT/'assembly-board-pose.json').write_text(json.dumps(pose)+'\n')
DOC.saveAs(str(OUT/'DailyMirror-P4-Open.FCStd'))
Part.makeCompound([r['obj'].Shape for r in parts]).exportStep(str(OUT/'Assembly.step'))
(OUT/'scene.json').write_text(json.dumps(scene))
(OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
report = dict(status='CAD validated; physical fit untested', freecad=App.Version()[:3], units='mm',
              layout=layout, checks=checks, parts=manifest)
(OUT/'validation.json').write_text(json.dumps(report, indent=2)+'\n')
print('BUILD_OK', len(parts), 'printable parts;', len(coupons), 'coupons;', len(checks), 'clearance checks')
print('LAYOUT', json.dumps({k: layout[k] for k in ('outer_mm', 'outer_with_ears_mm', 'lid_length',
      'lid_covers_board_mm', 'headroom_over_p4', 'max_led_height_above_perfboard', 'camera_lens_top_z')}))
App.closeDocument(DOC.Name)
