"""FreeCADCmd build.py. All dimensions mm; JSON is the coordinated-edit source.

Assembly axes: X across, Y up the wall, Z out from the wall. Exports are
individually rotated and grounded for printing; the FCStd stays assembled.
"""
import json
import math
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
SEG = C['rail_segment_length']
SEAM = 30.0
RLOW, RHIGH = SEAM - SEG, SEAM + SEG
CLOW, CHIGH = -24.0, 76.0
GAP = C['rail_clearance']
assert W >= 136 and H >= 165 and D >= 50, 'Layout requires at least 136 x 165 x 50'
assert 0.2 <= GAP <= 0.8, 'Use a realistic per-surface sliding allowance'
DOC = App.newDocument('DailyMirrorRailEnclosureV2')
PRINT = DOC.addObject('App::DocumentObjectGroup', 'PrintableParts')
REF = DOC.addObject('App::DocumentObjectGroup', 'HardwareReferences')
FIT = DOC.addObject('App::DocumentObjectGroup', 'FitCoupons')
PARAM = DOC.addObject('App::FeaturePython', 'DesignDimensions')
PARAM.addProperty('App::PropertyString', 'EditingInstructions')
PARAM.EditingInstructions = 'Edit dimensions.json and run build.sh; solids are not a live sketch feature tree.'
for k, value in C.items():
    PARAM.addProperty('App::PropertyLength', k, 'Source dimensions (mm)')
    setattr(PARAM, k, value)
    PARAM.setEditorMode(k, 1)
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

def profile(points, y, length):
    vertices = [V(CX+x, y, z) for x,z in points]
    return Part.Face(Part.makePolygon(vertices + [vertices[0]])).extrude(V(0, length, 0))

def hexagon(x, y, z, across_flats, height):
    radius = across_flats / math.sqrt(3)
    verts = [V(x+radius*math.cos(i*math.pi/3), y+radius*math.sin(i*math.pi/3), z) for i in range(6)]
    return Part.Face(Part.makePolygon(verts+[verts[0]])).extrude(V(0,0,height))

def add(name, shape, color, group=PRINT, orientation='back', note=''):
    shape = shape.removeSplitter()
    assert not shape.isNull() and shape.isValid(), name
    assert len(shape.Solids) == 1, (name, len(shape.Solids))
    obj = DOC.addObject('Part::Feature', name)
    obj.Shape = shape
    obj.addProperty('App::PropertyString', 'AssemblyNote')
    obj.AssemblyNote = note
    group.addObject(obj)
    if hasattr(obj, 'Visibility'):
        obj.Visibility = group == PRINT
    record = dict(name=name, obj=obj, color=color, orientation=orientation, note=note)
    (parts if group == PRINT else refs if group == REF else coupons).append(record)
    if App.GuiUp:
        obj.ViewObject.ShapeColor = color
        obj.ViewObject.Visibility = group == PRINT
    return shape

def dovetail_void(y, length):
    # Sides are offset horizontally by GAP; top has GAP normal clearance.
    # Continue the mouth below the rail base so the carriage has no bottom bridge.
    return profile([(-12-GAP,-2),(12+GAP,-2),(12+GAP,4),
                    (20+GAP,12),(20+GAP,12+GAP),(-20-GAP,12+GAP),
                    (-20-GAP,12),(-12-GAP,4)], y, length)

# SHELL: flat bed-facing rear, continuous corner screw towers, Pi standoffs.
case_screws = [(9,9),(W-9,9),(9,H-9),(W-9,H-9)]
pi_x, pi_y = 18.0, 58.0
pi_holes = [(pi_x+x,pi_y+y) for x,y in [(3.5,3.5),(61.5,3.5),(3.5,52.5),(61.5,52.5)]]
mount_holes = [(CX+x,y) for x in (-24,24) for y in (14,44)]
shell = roundbox(0,0,REAR,W,H,D,C['corner_radius']).cut(
    roundbox(T,T,REAR+T,W-2*T,H-2*T,D,C['corner_radius']-T))
shell = union([shell] +
    [cylinder(x,y,REAR+T,16,D-T) for x,y in case_screws] +
    [cylinder(x,y,REAR+T,6,8) for x,y in pi_holes])
cuts = [cylinder(x,y,FRONT-12,C['case_pilot'],13) for x,y in case_screws]
cuts += [cylinder(x,y,REAR+T+1,C['pi_pilot'],8) for x,y in pi_holes]
cuts += [cylinder(x,y,REAR-1,3.4,T+2) for x,y in mount_holes]
cuts += [cylinder(28,-1,REAR+21,C['power_entry_hole'],T+2,(0,1,0))]
# Slots bridge only 3 mm when printed back-down; airflow enters low and exits high.
for x in range(44,112,8):
    cuts += [box(x,-1,REAR+15,3,T+2,23)]
for x in range(28,116,8):
    cuts += [box(x,H-T-1,REAR+15,3,T+2,23)]
# Two tie saddles route a tie through a 2.5 mm-high tunnel along X.
saddles = []
for y in (18,32):
    saddle = box(20,y,REAR+T,16,6,5)
    saddle = saddle.cut(box(19,y+1.5,REAR+T+1,18,3,2.5))
    saddles.append(saddle)
shell = add('Housing', subtract(union([shell]+saddles), cuts), (.28,.34,.38),
            note='Print rear flat on bed. Pi: four M2.5 x 8 thread-forming screws. See guide for assembly order.')

# FRONT: all three circular openings and real raised camera screw posts.
cam_top = H-28.5
cam_lens_y = cam_top-C['camera_lens_from_top_edge']
cam_holes = [(CX+x,cam_top-y) for x in (-C['camera_hole_spacing_x']/2,C['camera_hole_spacing_x']/2)
             for y in (2,2+C['camera_hole_spacing_y'])]
cam_z = FRONT-C['camera_standoff_height']
led_x, led_y = CX+34, cam_lens_y
button_x, button_y = CX+30, 38.0
front = roundbox(0,0,FRONT,W,H,T,C['corner_radius'])
front = union([front]+[cylinder(x,y,cam_z,4.4,C['camera_standoff_height']) for x,y in cam_holes])
# Short locating tabs stay away from screws, Pi, camera, and control nuts.
front = union([front,box(45,3.4,FRONT-3,25,2,3),box(45,H-5.4,FRONT-3,25,2,3),
               box(3.4,75,FRONT-3,2,20,3),box(W-5.4,75,FRONT-3,2,20,3)])
# LED baffle leaves a standard 5 mm LED flange clear of the camera opening.
front = union([front, cylinder(led_x,led_y,FRONT-6,10,6).cut(cylinder(led_x,led_y,FRONT-7,7,8))])
cuts = [cylinder(x,y,FRONT-1,3.4,T+2) for x,y in case_screws]
cuts += [cylinder(CX,cam_lens_y,FRONT-1,C['camera_lens_hole'],T+2),
         cylinder(led_x,led_y,FRONT-1,C['led_hole'],T+2),
         cylinder(button_x,button_y,FRONT-1,C['button_hole'],T+2)]
cuts += [cylinder(x,y,cam_z-.1,C['camera_pilot'],6.1) for x,y in cam_holes]
front = add('FrontPanel', subtract(front,cuts), (.91,.90,.85), orientation='front',
            note='Print exterior face on bed, camera posts upward. Four M2 x 6 thread-forming screws from PCB rear into posts.')

# RAIL: two screw-mounted sections, with a shallow rear alignment tongue.
def rail_blank(y, length):
    return union([box(CX-40,y,0,80,length,4),
                  profile([(-12,4),(12,4),(20,12),(-20,12)], y, length)])

rail_lower = rail_blank(RLOW,SEG)
rail_lower = union([rail_lower,box(CX-8,SEAM-.1,0,16,6.1,2)])
rail_upper = rail_blank(SEAM,SEG)
keygap = C['rail_key_clearance']
rail_upper = rail_upper.cut(box(CX-8-keygap,SEAM-.1,-.1,16+2*keygap,6.1+keygap,2.1+keygap))
rails=[]
for name,shape,y0 in [('RailLower',rail_lower,RLOW),('RailUpper',rail_upper,SEAM)]:
    holes=[cylinder(CX+x,y0+yy,-1,4.5,6) for x in (-35,35) for yy in (18,SEG-18)]
    stop_y=RLOW+4 if name=='RailLower' else RHIGH-4
    holes += [cylinder(CX+x,stop_y,-1,C['case_pilot'],6) for x in (-27,27)]
    rails.append(add(name,subtract(shape,holes),(.49,.57,.61),
                     note='Back against bed/wall. Wall screw heads max 8 mm diameter. Align joint with carriage before tightening wall screws.'))

# CARRIAGE: full-length female dovetail; forward thumb lock below enclosure.
carriage = box(CX-30,CLOW,4+GAP,60,CHIGH-CLOW,REAR-4-GAP)
carriage = carriage.cut(dovetail_void(CLOW-1,CHIGH-CLOW+2))
lock_y = -13.5
cuts=[cylinder(x,y,REAR-9,C['case_pilot'],10) for x,y in mount_holes]
cuts += [cylinder(CX,lock_y,11,4.5,14), hexagon(CX,lock_y,17,7.2,3.5),
         box(CX-4.25,CLOW-1,17,8.5,lock_y-CLOW+1,3.5)]
# Brake puck pocket opens toward rail and holds a 1.5 mm pad under the screw tip.
cuts += [cylinder(CX,lock_y,11.9,C['brake_pad_diameter']+.5,3.1)]
carriage=add('Carriage',subtract(carriage,cuts),(.86,.47,.22),orientation='end',
             note='Print standing on its short end, 100 mm high, with brim. Insert M4 nut via lower end slot before assembling.')

# END STOPS are separately installed AFTER carriage insertion.
stops=[]
for name,y in [('StopLower',RLOW),('StopUpper',RHIGH-8)]:
    stop=box(CX-32,y,4,64,8,12).cut(dovetail_void(y-1,10))
    cutters=[cylinder(CX+x,y+4,3,3.4,14) for x in (-27,27)]
    cutters += [cylinder(CX+x,y+4,12,6.4,5) for x in (-27,27)]
    stops.append(add(name,subtract(stop,cutters),(.86,.47,.22),orientation='end',
                     note='Two M3 x 12 thread-forming screws each, seated in recessed holes.'))

# KNOB: front-loaded hex recess captures an ordinary M4 x 16 hex-head screw.
# Under-head bearing plane is z=29.0; tip reaches z=13.0 into the brake puck.
knob=union([cylinder(CX,lock_y,26,21,7)] +
           [cylinder(CX+9*math.cos(i*math.pi/3),lock_y+9*math.sin(i*math.pi/3),26,7,7) for i in range(6)])
knob=subtract(knob,[cylinder(CX,lock_y,25,4.4,9),hexagon(CX,lock_y,29,7.2,5)])
add('LockKnob',knob,(.86,.47,.22),note='M4 x 16 hex-head screw enters from knob front; captive M4 nut is in carriage.')
pad=add('BrakePad',cylinder(CX,lock_y,12.05,C['brake_pad_diameter'],C['brake_pad_thickness']),(.14,.16,.18),
        note='Print in TPU or cut a rubber disc from this template. Retain with a dab of flexible adhesive on screw tip only.')

# REFERENCES are clearance envelopes, deliberately excluded from print exports.
pcb=roundbox(pi_x,pi_y,REAR+T+8,85,56,1.6,3)
pcb=subtract(pcb,[cylinder(x,y,REAR+T+7,2.75,4) for x,y in pi_holes])
add('Pi4PCB',pcb,(.15,.48,.32),REF,note='Official Pi 4 B board outline and mounting hole pattern.')
add('PiComponentAllowance',box(pi_x,pi_y,REAR+T+9.6,88,56,26),(.17,.50,.35),REF,
    note='26 mm above PCB, including cooler and header plugs; simplified allowance, measure your installed cooler.')
camera=box(CX-12.5,cam_top-23.862,cam_z-1.6,25,23.862,1.6)
camera=subtract(camera,[cylinder(x,y,cam_z-2,2.2,3) for x,y in cam_holes])
add('CameraPCB',camera,(.18,.48,.33),REF,note='Standard PiCam mounting pattern, FPC connector toward enclosure bottom; actual sensor board unmeasured.')
add('CameraLensAllowance',cylinder(CX,cam_lens_y,cam_z,15,11),(.09,.12,.15),REF,
    note='Provisional 15 mm diameter x 11 mm lens envelope; hole position may differ between sensors.')
add('ButtonAllowance',cylinder(button_x,button_y,FRONT-25,18,24),(.34,.41,.47),REF,
    note='Placeholder 12 mm panel-mount button with <=18 mm body/nut and 25 mm internal depth.')
add('ButtonCap',cylinder(button_x,button_y,FRONT+T,15,2),(.12,.16,.19),REF)
add('LEDAllowance',cylinder(led_x,led_y,FRONT-1,5,5),(.82,.87,.65),REF)
add('FutureFlashReserve',box(W-39,77,FRONT-23,24,28,20),(.87,.71,.26),REF,
    note='Planning space only; future flash requires a revised front panel, optics, driver and thermal design.')
add('PowerPlugAllowance',box(pi_x+11.2-5.5,28,REAR+T+8,11,30,8),(.23,.29,.32),REF,
    note='Straight USB-C plug body allowance; max 13 mm plug cross-section must pass 14 mm bottom opening.')
add('LockScrew',union([cylinder(CX,lock_y,13,4,16),hexagon(CX,lock_y,29,7,2.8)]),(.63,.66,.68),REF)
add('LockNut',hexagon(CX,lock_y,17.15,7,3.2).cut(cylinder(CX,lock_y,17,4,4)),(.63,.66,.68),REF)
# Reference fasteners clarify the two real screw connections. Simplified shafts
# show nominal major diameters; printed pilots are smaller for thread forming.
for i,(x,y) in enumerate(mount_holes,1):
    add('RearMountScrew%d'%i,union([cylinder(x,y,REAR+T-10,3,10),
        cylinder(x,y,REAR+T,5.6,2)]),(.73,.77,.79),REF,
        note='M3 x 10 reference: inserted from INSIDE the housing, through its 3 mm rear wall, into the carriage. Not printable.')
for i,(x,y) in enumerate(pi_holes,1):
    bearing=REAR+T+8+1.6
    add('PiMountScrew%d'%i,union([cylinder(x,y,bearing-8,2.5,8),
        cylinder(x,y,bearing,4.5,1.5)]),(.73,.77,.79),REF,
        note='M2.5 x 8 reference: through an existing Pi PCB hole into the printed post. Not printable.')

# Small trials use IDENTICAL mating profiles / actual camera standoffs.
add('RailFitMale',rail_blank(0,30),(.49,.57,.61),FIT)
fitfemale=box(CX-30,0,4+GAP,60,24,REAR-4-GAP).cut(dovetail_void(-1,26))
add('RailFitFemale',fitfemale,(.86,.47,.22),FIT,orientation='end')
cam_coupon=front.common(box(CX-20,cam_top-29,FRONT-15,40,36,20))
add('CameraFitCoupon',cam_coupon,(.91,.90,.85),FIT,orientation='front')

DOC.recompute()

def print_shape(record):
    s=record['obj'].Shape.copy()
    if record['orientation']=='front':
        s.rotate(V(0,0,0),V(1,0,0),180)
    if record['orientation']=='end':
        s.rotate(V(0,0,0),V(1,0,0),90)
    b=s.BoundBox
    s.translate(V(-b.XMin,-b.YMin,-b.ZMin))
    return s

scene,manifest=[],[]
for rec in parts+refs+coupons:
    obj=rec['obj']
    s=obj.Shape
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
    # STEP preserves assembly placement; STL is independently bed-oriented.
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
clearance('Housing / carriage',shell,carriage)
clearance('Rail joint',rails[0],rails[1])
for i,a in enumerate(parts):
    for b in parts[i+1:]:
        clearance(a['name']+' / '+b['name'],a['obj'].Shape,b['obj'].Shape)
for rail in rails:
    clearance('Carriage / rail',carriage,rail)
for ref in refs:
    if ref['name'] in ('Pi4PCB','PiComponentAllowance','CameraPCB','CameraLensAllowance',
                       'ButtonAllowance','PowerPlugAllowance','FutureFlashReserve'):
        clearance(ref['name']+' / housing',ref['obj'].Shape,shell)
        clearance(ref['name']+' / front',ref['obj'].Shape,front)
# Sweep the unlocked carriage through the entire travel and over the rail seam.
travel_low=RLOW+8-CLOW
travel_high=RHIGH-8-CHIGH
for offset in (travel_low,travel_low+.1,-50,0,50,travel_high-.1,travel_high):
    moved=carriage.copy(); moved.translate(V(0,offset,0))
    for i,obstacle in enumerate(rails+stops):
        clearance('Travel %.2f / obstacle %d'%(offset,i),moved,obstacle)
# End stops obstruct further travel; dovetail prevents pulling off the wall.
for offset,stop in ((travel_low-1,stops[0]),(travel_high+1,stops[1])):
    moved=carriage.copy(); moved.translate(V(0,offset,0))
    assert moved.common(stop).Volume>.1,'Stop does not retain carriage'
pulled=carriage.copy(); pulled.translate(V(0,0,2))
assert sum(pulled.common(r).Volume for r in rails)>.1,'Dovetail fails to retain carriage'
if App.GuiUp:
    import FreeCADGui as Gui
    Gui.activeDocument().activeView().viewAxonometric()
    Gui.activeDocument().activeView().fitAll()
DOC.recompute()
DOC.saveAs(str(OUT/'DailyMirror-Rail-Enclosure-v2.FCStd'))
Part.makeCompound([r['obj'].Shape for r in parts]).exportStep(str(OUT/'Assembly.step'))
(OUT/'scene.json').write_text(json.dumps(scene))
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
report=dict(status='CAD validated; physical fit, sliding friction and load capacity untested',
            freecad=App.Version()[:3],units='mm',outer_mm=[W,H,D+T],
            rail_length_mm=SEG*2,travel_mm=travel_high-travel_low,
            largest_bed_dimension_mm=max(max(m['print_size_mm'][:2]) for m in manifest),
            checks=checks,retention_checks_passed=True,parts=manifest)
(OUT/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
print('BUILD_OK',len(parts),'printable parts;',len(coupons),'coupons;',len(checks),'clearance checks; travel',report['travel_mm'])
if not App.GuiUp:
    App.closeDocument(DOC.Name)
