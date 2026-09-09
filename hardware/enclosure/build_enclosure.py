"""Run with FreeCADCmd or exec() in FreeCAD's Python console. Units: mm."""
import json
from pathlib import Path
import FreeCAD as App
import Part
import MeshPart

OUT = Path(__file__).resolve().parent / 'output'
OUT.mkdir(exist_ok=True)
DOC = App.newDocument('DailyMirrorEnclosure')
W, H, D, T = 180.0, 180.0, 70.0, 3.0
parts = []
references = []

def hide(obj):
    if App.GuiUp:
        obj.ViewObject.Visibility = False

def box(name, x, y, z, w, h, d):
    obj = DOC.addObject('Part::Box', name)
    obj.Length, obj.Width, obj.Height = w, h, d
    obj.Placement.Base = App.Vector(x, y, z)
    return obj

def cyl(name, x, y, z, r, h):
    obj = DOC.addObject('Part::Cylinder', name)
    obj.Radius, obj.Height = r, h
    obj.Placement.Base = App.Vector(x, y, z)
    return obj

def fuse(name, objs):
    obj = DOC.addObject('Part::MultiFuse', name)
    obj.Shapes = objs
    for child in objs: hide(child)
    return obj

def cut(name, base, tools):
    tool = tools[0] if len(tools) == 1 else fuse(name+'Tools', tools)
    obj = DOC.addObject('Part::Cut', name)
    obj.Base, obj.Tool = base, tool
    hide(base); hide(tool)
    return obj

def finish(obj, label, color, ref=False):
    obj.Label = label
    obj.addProperty('App::PropertyString', 'DesignStatus')
    obj.DesignStatus = 'CONCEPT — hardware fit and print settings unverified'
    if App.GuiUp:
        obj.ViewObject.ShapeColor = color
    (references if ref else parts).append((obj, color))
    return obj

# Back is z=0, front z=70; y is vertical when wall-mounted.
outer = box('OuterShell', 0,0,0,W,H,D)
inner = box('InteriorVoid', T,T,T,W-2*T,H-2*T,D)
shell = cut('HollowShell', outer, [inner])
# Corner lid screws use pilot bores; final screw choice requires fit coupon.
bosses = [cyl('LidBoss',x,y,D-14,5,14) for x,y in [(7,7),(W-7,7),(7,H-7),(W-7,H-7)]]
# Pi PCB 85x56, mounting centres 58x49 from official Pi 4 drawing.
pi_x, pi_y = 20, 28
pi_holes = [(pi_x+x,pi_y+y) for x,y in [(3.5,3.5),(61.5,3.5),(3.5,52.5),(61.5,52.5)]]
bosses += [cyl('PiStandoff',x,y,T,3.5,9) for x,y in pi_holes]
shell = fuse('ShellWithStandoffs', [shell]+bosses)
tools = [cyl('LidPilot',x,y,D-12,1.25,13) for x,y in [(7,7),(W-7,7),(7,H-7),(W-7,H-7)]]
tools += [cyl('PiPilot',x,y,T+2,1.05,10) for x,y in pi_holes]
# Generous open-bottom service mouth: temporary cable routing, not exact ports.
tools += [box('CableServiceOpening',20,-1,15,75,T+2,30)]
for x in range(24,160,12):
    tools.append(box('UpperVent',x,H-T-1,22,6,T+2,30))
for x in range(110,160,12):
    tools.append(box('LowerVent',x,-1,22,6,T+2,30))
# Rear M4 clearance pattern for a future removable carriage/wall adapter.
rear_holes = [(70,90),(110,90),(70,130),(110,130)]
tools += [cyl('RearMountHole',x,y,-1,2.2,T+2) for x,y in rear_holes]
base = finish(cut('Base',shell,tools),'01 • Roomy base 180 × 180 × 70',(0.72,0.77,0.82))

# Lid and replaceable camera adapter; no assumed button or LED diameter.
lid = box('LidBlank',0,0,D,W,H,T)
lt = [cyl('LidClearance',x,y,D-1,1.7,T+2) for x,y in [(7,7),(W-7,7),(7,H-7),(W-7,H-7)]]
lt += [box('CameraOpening',66,114,D-1,48,48,T+2)]
cam_panel_holes = [(62,110),(118,110),(62,166),(118,166)]
lt += [cyl('AdapterPilot',x,y,D-1,1.05,T+2) for x,y in cam_panel_holes]
lid = finish(cut('Lid',lid,lt),'02 • Lid — controls remain undrilled',(0.9,0.91,0.9))
adapter = box('AdapterBlank',58,106,D+T,64,64,3)
at = [cyl('LensOpening',90,138,D+T-1,9,5)]
at += [cyl('AdapterClearance',x,y,D+T-1,1.3,5) for x,y in cam_panel_holes]
# Provisional 21 x 12.5 mm camera hole pattern, to be measured before printing.
cam_holes = [(90+x,138+y) for x,y in [(-10.5,-6.25),(10.5,-6.25),(-10.5,6.25),(10.5,6.25)]]
at += [cyl('CameraScrewClearance',x,y,D+T-1,1.2,5) for x,y in cam_holes]
adapter = finish(cut('CameraAdapter',adapter,at),'03 • Camera plate — provisional hole pattern',(0.22,0.28,0.34))

# Reference volumes are visual keep-outs, never exported as printable parts.
finish(box('PiPCB',pi_x,pi_y,12,85,56,1.6),'REFERENCE • Pi 4 board envelope',(0.13,0.5,0.34),True)
finish(box('PiCoolerKeepout',pi_x,pi_y,13.6,85,56,30),'REFERENCE • component/cooler allowance',(0.25,0.58,0.46),True)
finish(box('WiringReserve',115,28,8,48,65,45),'REFERENCE • wiring/perfboard reserve',(0.94,0.65,0.23),True)
finish(box('CameraPCB',77.5,126,64,25,24,1.6),'REFERENCE • unconfirmed camera PCB',(0.2,0.5,0.37),True)
finish(cyl('CameraLens',90,138,65.6,7,11),'REFERENCE • unconfirmed lens envelope',(0.12,0.15,0.18),True)
# Flat rear zones for chosen adhesive strips; leave downward removal access.
for x in [15,135]:
    finish(box('AdhesiveZone',x,20,-1,30,130,1),'REFERENCE • adhesive land, select actual strips',(0.2,0.65,0.83),True)

# Separate future slider study: captive C-channel, 300 mm carriage travel.
# Study geometry intentionally separate from print exports until lock/stop validation.
rail = box('RailBack',230,0,0,60,420,5)
rail = fuse('RailChannel',[rail,box('RailLeft',230,0,5,5,420,12),box('RailRight',285,0,5,5,420,12),box('RailLipLeft',230,0,17,12,420,4),box('RailLipRight',278,0,17,12,420,4),box('RailBottomStop',235,0,5,50,10,12),box('RailTopStop',235,410,5,50,10,12)])
finish(rail,'STUDY • 420 mm rail, removable top stop required',(0.4,0.47,0.55),True)
carriage = box('Carriage',235.5,160,5.5,49,100,10.8)
neck = box('CarriageNeck',244,160,16.3,32,100,10)
face = box('CarriageFace',225,160,26.3,70,100,5)
carriage = fuse('CarriageAssembly',[carriage,neck,face])
carriage = cut('CarriageMounting',carriage,[cyl('CarriageHole',x,y,25,2.2,8) for x,y in [(240,190),(280,190),(240,230),(280,230)]])
finish(carriage,'STUDY • carriage, clamp/detent not yet designed',(0.9,0.57,0.24),True)

DOC.recompute()
report = {'freecad_version': App.Version()[:3], 'units':'mm', 'outer_mm':[W,H,D+T], 'interior_mm':[W-2*T,H-2*T,D-T], 'camera_plate_extra_depth_mm':3, 'parts':[], 'status':'concept; not hardware-fit verified', 'slider_travel_mm':300}
scene = []
for obj,color in parts+references:
    shape=obj.Shape
    assert not shape.isNull() and shape.isValid(), obj.Name
    assert len(shape.Solids)==1, (obj.Name,len(shape.Solids))
    mesh = MeshPart.meshFromShape(Shape=shape,LinearDeflection=0.3,AngularDeflection=0.35,Relative=False)
    verts, facets = mesh.Topology
    scene.append({'name':obj.Name,'label':obj.Label,'color':color,'reference':(obj,color) in references,'vertices':[[v.x,v.y,v.z] for v in verts],'faces':[list(f) for f in facets]})
    if (obj,color) in parts:
        mesh.write(str(OUT/(obj.Name+'.stl')))
        step_path = OUT / (obj.Name + '.step')
        shape.exportStep(str(step_path))
        step_path.write_text('\n'.join(line.rstrip() for line in step_path.read_text().splitlines()) + '\n')
        report['parts'].append({'name':obj.Name,'valid':True,'solid_count':len(shape.Solids),'volume_mm3':round(shape.Volume,2),'mesh_closed':mesh.isSolid()})
        assert mesh.isSolid(), obj.Name
# Hide intermediate construction objects even in headless saved documents.
final_names={o.Name for o,c in parts+references}
for obj in DOC.Objects:
    if hasattr(obj,'Visibility'): obj.Visibility=obj.Name in final_names
if App.GuiUp:
    import FreeCADGui as Gui
    for obj,c in references: obj.ViewObject.Transparency=65
    Gui.activeDocument().activeView().viewAxonometric()
    Gui.activeDocument().activeView().fitAll()
DOC.recompute()
DOC.saveAs(str(OUT/'DailyMirrorEnclosure.FCStd'))
(OUT/'validation.json').write_text(json.dumps(report,indent=2))
(OUT/'scene.json').write_text(json.dumps(scene))
print('ENCLOSURE_BUILD_OK '+json.dumps(report))
App.closeDocument(DOC.Name) if not App.GuiUp else None
