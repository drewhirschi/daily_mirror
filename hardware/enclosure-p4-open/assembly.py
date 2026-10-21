"""FreeCADCmd assembly.py. Builds output/FullAssembly.FCStd for looking at, not printing.

Tray and CameraLid from build.py, with stand-ins for the perfboard, female
headers, camera, LED and button. The Waveshare board itself is imported by
OpenAssembly.FCMacro in desktop FreeCAD, because STEP colours only survive a
GUI import. Run build.py first.
"""
import json
from pathlib import Path
import FreeCAD as App
import Part

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
V = App.Vector
C = json.loads((ROOT / 'dimensions.json').read_text())
lay = json.loads((OUT / 'layout.json').read_text())
src = App.openDocument(str(OUT / 'DailyMirror-P4-Open.FCStd'))
get = lambda name: src.getObject(name).Shape.copy()

DOC = App.newDocument('DailyMirrorP4OpenAssembly')
groups = {}
def add(name, shape, color, group, transparency=0, visible=True, note=''):
    if group not in groups:
        groups[group] = DOC.addObject('App::DocumentObjectGroup', group)
    obj = DOC.addObject('Part::Feature', name)
    obj.Shape = shape
    for prop, kind, value in (('DisplayColor', 'App::PropertyString', json.dumps(color)),
                              ('DisplayTransparency', 'App::PropertyInteger', transparency),
                              ('DisplayVisible', 'App::PropertyBool', visible),
                              ('AssemblyNote', 'App::PropertyString', note)):
        obj.addProperty(kind, prop)
        setattr(obj, prop, value)
    groups[group].addObject(obj)

def cyl(x, y, z, d, h, axis=(0, 0, 1)):
    return Part.makeCylinder(d / 2, h, V(x, y, z), V(*axis))

add('Tray', get('Tray'), (.28, .34, .38), 'Printed', 15)
add('CameraLid', get('CameraLid'), (.91, .90, .85), 'Printed', 55)

# Perfboard with a 2.54 mm hole grid; corner holes are the unmeasured guess.
pb = get('Perfboard').BoundBox
grid = [cyl(pb.XMin + 2.54 * (i + 1.5), pb.YMin + 2.54 * (j + 1.5), pb.ZMin - 1, 1.0, 4)
        for i in range(int(pb.XLength / 2.54) - 2) for j in range(int(pb.YLength / 2.54) - 2)]
add('Perfboard', get('Perfboard').cut(Part.makeCompound(grid)), (.80, .62, .32), 'Electronics',
    note='Corner holes are the 2 mm-inset guess from dimensions.json, not measured.')
for side in ('Left', 'Right'):
    add('FemaleHeader' + side, get('FemaleHeader' + side), (.10, .10, .10), 'Electronics')

# Camera: Arducam IMX519 on the outside of the lid, lens facing out.
add('CameraPCB', get('CameraPCB'), (.10, .35, .20), 'Electronics', note='25 x 24 mm, real hole pattern.')
lb = get('CameraLens').BoundBox
add('CameraLens', get('CameraLens').fuse(cyl(lb.Center.x, lb.Center.y, lb.ZMax, 7.0, 1.5)),
    (.12, .12, .12), 'Electronics', note='Approximate autofocus lens block.')
add('CameraConnector', get('CameraConnector'), (.85, .85, .80), 'Electronics',
    note='15-pin connector on the camera back, facing the open tray.')

# LED (5 mm, domed) and button (12 mm tactile with round cap), placeholders.
lb = get('LEDAllowance').BoundBox
r = C['led_diameter'] / 2 - .4
led = cyl(lb.Center.x, lb.Center.y, lb.ZMax - 9, 2 * r, 9 - r).fuse(
    Part.makeSphere(r, V(lb.Center.x, lb.Center.y, lb.ZMax - r)))
led = led.fuse(cyl(lb.Center.x, lb.Center.y, lb.ZMax - 9, 2 * r + .8, 1))
legs = Part.makeCompound([cyl(lb.Center.x + dx, lb.Center.y, lb.ZMin, .5, lb.ZLength - 9)
                          for dx in (-1.9, -.63, .63, 1.9)])
add('RGBLED', led.fuse(legs), (.95, .95, .90), 'Electronics', 20, note='Placeholder height: led_height in dimensions.json.')
bb = get('ButtonAllowance').BoundBox
btn = Part.makeBox(12, 12, 3.5, V(bb.Center.x - 6, bb.Center.y - 6, bb.ZMin)).fuse(
    cyl(bb.Center.x, bb.Center.y, bb.ZMin + 3.5, 6.8, bb.ZLength - 3.5))
add('Button', btn, (.20, .20, .22), 'Electronics')

# Space reservations from build.py, hidden by default.
for name in ('CSIRibbonRiser', 'CSIRibbonAllowance', 'PowerPlugAllowance', 'LEDAllowance', 'ButtonAllowance', 'SolderSide'):
    add(name, get(name), (.85, .75, .35), 'Allowances', 50, visible=name.startswith('CSI'))

App.closeDocument(src.Name)
DOC.recompute()
DOC.saveAs(str(OUT / 'FullAssembly.FCStd'))
print('ASSEMBLY_OK', len(DOC.Objects), 'objects')
