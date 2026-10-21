"""FreeCADCmd build.py. All dimensions mm; JSON is the coordinated-edit source.

Housing for the Waveshare ESP32-P4-WIFI6 plugged into a 70 x 50 perfboard,
mounting on the unchanged enclosure-v2 rail carriage. Same assembly axes as v2:
X across, Y up the wall, Z out from the wall; housing rear outer face at Z=23.
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
V = App.Vector
W, H, D, T = (C[k] for k in ('width', 'height', 'body_depth', 'wall'))
CX, REAR = W / 2, 23.0
FRONT = REAR + D
assert W >= 80 and H >= 145 and D >= 46, 'Layout requires at least 80 x 145 x 46'
DOC = App.newDocument('DailyMirrorP4Enclosure')
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

def add(name, shape, color, group=PRINT, orientation='back', note=''):
    shape = shape.removeSplitter()
    assert not shape.isNull() and shape.isValid(), name
    assert len(shape.Solids) == 1, (name, len(shape.Solids))
    obj = DOC.addObject('Part::Feature', name)
    obj.Shape = shape
    obj.addProperty('App::PropertyString', 'AssemblyNote')
    obj.AssemblyNote = note
    group.addObject(obj)
    record = dict(name=name, obj=obj, color=color, orientation=orientation, note=note)
    (parts if group == PRINT else refs if group == REF else coupons).append(record)
    return shape

# LAYOUT. Perfboard portrait, P4 plugged into it USB-end down so the power
# lead drops straight out of the bottom; camera above the stack on the front.
PBW, PBH, INSET = C['perfboard_width'], C['perfboard_height'], C['perfboard_hole_inset']
pb_x, pb_y = CX - PBW/2, 50.0
pb_z = REAR + T + C['perfboard_post_height']
pb_holes = [(pb_x+x, pb_y+y) for x in (INSET, PBW-INSET) for y in (INSET, PBH-INSET)]
p4_cx, p4_y = CX - 12.0, 48.0           # USB-C end of the 71 x 21 board
p4_z = pb_z + 1.6 + C['female_header_height'] + C['male_header_spacer']
usb_z = p4_z + 1.6 + 1.2                 # mid-mount USB-C centre
csi_y = p4_y + 36.1
case_screws = [(9,9),(W-9,9),(9,H-9),(W-9,H-9)]
mount_holes = [(CX+x,y) for x in (-24,24) for y in (14,44)]   # v2 carriage pattern
cam_top = H - 14.0
cam_lens_y = cam_top - C['camera_lens_from_top_edge']
cam_holes = [(CX+x,cam_top-y) for x in (-C['camera_hole_spacing_x']/2,C['camera_hole_spacing_x']/2)
             for y in (2,2+C['camera_hole_spacing_y'])]
cam_z = FRONT - C['camera_standoff_height']
led_x, led_y = CX + 28, cam_lens_y
button_x, button_y = CX + 22, 30.0

# SHELL: flat rear on the bed, corner screw towers, perfboard posts.
shell = roundbox(0,0,REAR,W,H,D,C['corner_radius']).cut(
    roundbox(T,T,REAR+T,W-2*T,H-2*T,D,C['corner_radius']-T))
shell = union([shell] +
    [cylinder(x,y,REAR+T,16,D-T) for x,y in case_screws] +
    [cylinder(x,y,REAR+T,6,C['perfboard_post_height']) for x,y in pb_holes])
cuts = [cylinder(x,y,FRONT-12,C['case_pilot'],13) for x,y in case_screws]
cuts += [cylinder(x,y,REAR+T+1,C['perfboard_pilot'],C['perfboard_post_height']) for x,y in pb_holes]
cuts += [cylinder(x,y,REAR-1,3.4,T+2) for x,y in mount_holes]
cuts += [cylinder(p4_cx,-1,usb_z,C['power_entry_hole'],T+2,(0,1,0))]
for x in (48,56,64):
    cuts += [box(x,-1,REAR+15,3,T+2,23)]
for x in range(22,67,8):
    cuts += [box(x,H-T-1,REAR+15,3,T+2,23)]
shell = add('Housing', subtract(shell, cuts), (.28,.34,.38),
            note='Print rear flat on bed. Perfboard: four M2 x 6 thread-forming screws. Carriage: four M3 x 10 from inside.')

# FRONT: lens, LED, button and raised camera posts (same as v2).
front = roundbox(0,0,FRONT,W,H,T,C['corner_radius'])
front = union([front]+[cylinder(x,y,cam_z,4.4,C['camera_standoff_height']) for x,y in cam_holes])
front = union([front,box(CX-12.5,3.4,FRONT-3,25,2,3),box(CX-12.5,H-5.4,FRONT-3,25,2,3),
               box(3.4,75,FRONT-3,2,20,3),box(W-5.4,75,FRONT-3,2,20,3)])
front = union([front, cylinder(led_x,led_y,FRONT-6,10,6).cut(cylinder(led_x,led_y,FRONT-7,7,8))])
cuts = [cylinder(x,y,FRONT-1,3.4,T+2) for x,y in case_screws]
cuts += [cylinder(CX,cam_lens_y,FRONT-1,C['camera_lens_hole'],T+2),
         cylinder(led_x,led_y,FRONT-1,C['led_hole'],T+2),
         cylinder(button_x,button_y,FRONT-1,C['button_hole'],T+2)]
cuts += [cylinder(x,y,cam_z-.1,C['camera_pilot'],6.1) for x,y in cam_holes]
front = add('FrontPanel', subtract(front,cuts), (.91,.90,.85), orientation='front',
            note='Print exterior face on bed, camera posts upward. Four M2 x 6 thread-forming screws from PCB rear into posts.')

# REFERENCES: clearance envelopes only, not exported for printing.
perf = box(pb_x,pb_y,pb_z,PBW,PBH,1.6)
perf = subtract(perf,[cylinder(x,y,pb_z-1,2.0,4) for x,y in pb_holes])
add('Perfboard',perf,(.70,.55,.25),REF,note='70 x 50 perfboard; hole inset from dimensions.json - measure yours.')
add('SolderSide',box(pb_x,pb_y,pb_z-2.5,PBW,PBH,2.5),(.55,.45,.25),REF,
    note='2.5 mm allowance for solder joints and wire under the perfboard.')
add('HeaderStack',box(p4_cx-10.5,p4_y+4,pb_z+1.6,21,49.5,p4_z-pb_z-1.6),(.15,.15,.15),REF,
    note='Two 1x20 female headers on the perfboard with the P4 header plastic seated on top.')
add('P4Board',box(p4_cx-10.5,p4_y,p4_z,21,71,1.6),(.15,.48,.32),REF,
    note='Waveshare ESP32-P4-WIFI6 71 x 21 mm, USB-C end down, components facing the front panel.')
add('P4Components',box(p4_cx-10.5,p4_y,p4_z+1.6,21,71,4.7),(.17,.50,.35),REF,
    note='Tallest top part is the 4.7 mm OTG connector; CSI/DSI FPC connectors are 4.1 mm, vertical.')
add('CSIRibbonAllowance',box(p4_cx-9,csi_y-2,p4_z+5.7,18,cam_top-23.862-csi_y+4,cam_z-1.6-4-p4_z-5.7),
    (.85,.75,.35),REF,note='Space for the 22-to-15 ribbon to rise from the vertical CSI connector and turn up to the camera.')
add('PowerPlugAllowance',box(p4_cx-6,16,usb_z-4,12,p4_y-1.3-16,8),(.23,.29,.32),REF,
    note='Straight USB-C plug body; overmold must pass the 14 mm bottom opening.')
camera=box(CX-12.5,cam_top-23.862,cam_z-1.6,25,23.862,1.6)
camera=subtract(camera,[cylinder(x,y,cam_z-2,2.2,3) for x,y in cam_holes])
add('CameraPCB',camera,(.18,.48,.33),REF,note='Arducam IMX519, 25 x 24 mm, 21 x 12.5 hole pitch; ribbon connector toward the bottom.')
add('CameraLensAllowance',cylinder(CX,cam_lens_y,cam_z,15,11),(.09,.12,.15),REF)
add('ButtonAllowance',cylinder(button_x,button_y,FRONT-25,18,24),(.34,.41,.47),REF)
add('ButtonCap',cylinder(button_x,button_y,FRONT+T,15,2),(.12,.16,.19),REF)
add('LEDAllowance',cylinder(led_x,led_y,FRONT-1,5,5),(.82,.87,.65),REF)
add('CarriageEnvelope',box(CX-30,-24,4.4,60,100,REAR-4.4),(.86,.47,.22),REF,
    note='Bounding box of the enclosure-v2 Carriage; it is not reprinted.')
add('LockKnobEnvelope',cylinder(CX,-13.5,26,25,7),(.86,.47,.22),REF)

# COUPONS: print these first, they take minutes.
cam_coupon=front.common(box(CX-20,cam_top-29,FRONT-15,40,36,20))
add('CameraFitCoupon',cam_coupon,(.91,.90,.85),FIT,orientation='front')
plate=box(pb_x-3,pb_y-3,REAR,PBW+6,PBH+6,2)
plate=plate.cut(box(pb_x+8,pb_y+8,REAR-1,PBW-16,PBH-16,4))
plate=union([plate]+[cylinder(x,y,REAR+2,6,C['perfboard_post_height']-1) for x,y in pb_holes])
plate=subtract(plate,[cylinder(x,y,REAR+1,C['perfboard_pilot'],C['perfboard_post_height']+1) for x,y in pb_holes])
add('PerfboardFitCoupon',plate,(.28,.34,.38),FIT,
    note='Same post pattern as the housing. Screw the perfboard on to check the hole spacing.')

DOC.recompute()

def print_shape(record):
    s=record['obj'].Shape.copy()
    if record['orientation']=='front':
        s.rotate(V(0,0,0),V(1,0,0),180)
    b=s.BoundBox
    s.translate(V(-b.XMin,-b.YMin,-b.ZMin))
    return s

scene,manifest=[],[]
for rec in parts+refs+coupons:
    s=rec['obj'].Shape
    mesh=MeshPart.meshFromShape(Shape=s,LinearDeflection=.12,AngularDeflection=.2,Relative=False)
    vertices,faces=mesh.Topology
    kind='part' if rec in parts else 'reference' if rec in refs else 'coupon'
    scene.append(dict(name=rec['name'],kind=kind,color=rec['color'],note=rec['note'],
                      vertices=[[p.x,p.y,p.z] for p in vertices],faces=[list(f) for f in faces]))
    if kind=='reference':
        continue
    ps=print_shape(rec)
    pm=MeshPart.meshFromShape(Shape=ps,LinearDeflection=.08,AngularDeflection=.16,Relative=False)
    assert pm.isSolid(), rec['name']
    pm.write(str(OUT/(rec['name']+'.stl')))
    s.exportStep(str(OUT/(rec['name']+'.step')))
    b=ps.BoundBox
    manifest.append(dict(name=rec['name'],kind=kind,solid_count=len(s.Solids),valid=s.isValid(),
                         closed_mesh=pm.isSolid(),volume_mm3=round(s.Volume,3),
                         print_size_mm=[round(v,3) for v in (b.XLength,b.YLength,b.ZLength)],
                         print_orientation=rec['orientation'],note=rec['note']))

checks=[]
def clearance(name,a,b,allowance=1e-5):
    v=a.common(b).Volume
    checks.append(dict(name=name,overlap_mm3=round(v,8),passed=v<allowance))
    assert v<allowance,(name,v)

clearance('Housing / front',shell,front)
named={r['name']:r['obj'].Shape for r in refs}
for n in ('Perfboard','HeaderStack','P4Board','P4Components','CSIRibbonAllowance',
          'PowerPlugAllowance','CameraPCB','CameraLensAllowance','ButtonAllowance'):
    clearance(n+' / housing',named[n],shell)
    clearance(n+' / front',named[n],front)
for n in ('CarriageEnvelope','LockKnobEnvelope'):
    clearance(n+' / housing',named[n],shell)
clearance('SolderSide / front',named['SolderSide'],front)
clearance('P4 stack / button',named['P4Components'],named['ButtonAllowance'])
clearance('Power plug / button',named['PowerPlugAllowance'],named['ButtonAllowance'])
clearance('Ribbon / camera',named['CSIRibbonAllowance'],named['CameraPCB'])
# Cable leaving the bottom must clear the lock knob below the housing.
cable=cylinder(p4_cx,-40,usb_z,6,40,(0,1,0))
clearance('Power cable / knob',cable,named['LockKnobEnvelope'])
clearance('Power cable / carriage',cable,named['CarriageEnvelope'])

DOC.saveAs(str(OUT/'DailyMirror-P4-Enclosure.FCStd'))
Part.makeCompound([r['obj'].Shape for r in parts]).exportStep(str(OUT/'Assembly.step'))
(OUT/'scene.json').write_text(json.dumps(scene))
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
report=dict(status='CAD validated; physical fit untested',freecad=App.Version()[:3],units='mm',
            outer_mm=[W,H,D+T],checks=checks,parts=manifest)
(OUT/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
print('BUILD_OK',len(parts),'printable parts;',len(coupons),'coupons;',len(checks),'clearance checks')
App.closeDocument(DOC.Name)
