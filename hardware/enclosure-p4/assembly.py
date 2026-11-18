"""FreeCADCmd assembly.py. Builds output/FullAssembly.FCStd for looking at, not printing.

Puts the P4 housing on the enclosure-v2 carriage and rail, with more realistic
stand-ins for the perfboard, female headers and camera. The Waveshare board
itself is imported by OpenAssembly.FCMacro in desktop FreeCAD, because STEP
colours only survive a GUI import. Run build.py and enclosure-v2/build.sh first.
"""
import json
from pathlib import Path
import FreeCAD as App
import Part

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
V2 = ROOT.parent / 'enclosure-v2'
V = App.Vector
v2dims = json.loads((V2 / 'dimensions.json').read_text())
# The v2 carriage is centred on the 140 mm v2 housing; this housing is narrower.
SHIFT = V((v2dims['width'] - json.loads((ROOT / 'dimensions.json').read_text())['width']) / 2, 0, 0)

src = App.openDocument(str(OUT / 'DailyMirror-P4-Enclosure.FCStd'))
def p4(name):
    s = src.getObject(name).Shape.copy()
    s.translate(SHIFT)
    return s

DOC = App.newDocument('DailyMirrorFullAssembly')
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

# Printed P4 parts, see-through so the inside is visible.
add('Housing', p4('Housing'), (.28, .34, .38), 'Printed', 55)
add('FrontPanel', p4('FrontPanel'), (.91, .90, .85), 'Printed', 55)

# Rail and carriage from enclosure-v2, unchanged.
for name, color in (('Carriage', (.86, .47, .22)), ('LockKnob', (.86, .47, .22)),
                    ('BrakePad', (.15, .15, .15)), ('StopLower', (.86, .47, .22)),
                    ('StopUpper', (.86, .47, .22)), ('RailKey', (.95, .75, .20))):
    add(name, Part.read(str(V2 / 'output' / (name + '.step'))), color, 'RailAndCarriage')
rail = Part.read(str(V2 / 'output' / 'RailSection.step'))
for i in range(v2dims['rail_sections']):
    add('RailSection%d' % (i + 1), rail.translated(V(0, i * v2dims['rail_segment_length'], 0)),
        (.49, .57, .61), 'RailAndCarriage')

# Perfboard: 2.54 mm hole grid, corner holes where the housing posts are (unmeasured).
pb = p4('Perfboard').BoundBox
grid = [cyl(pb.XMin + 2.54 * (i + 1.5), pb.YMin + 2.54 * (j + 1.5), pb.ZMin - 1, 1.0, 4)
        for i in range(int(pb.XLength / 2.54) - 2) for j in range(int(pb.YLength / 2.54) - 2)]
add('Perfboard', p4('Perfboard').cut(Part.makeCompound(grid)), (.80, .62, .32), 'Electronics',
    note='Corner holes are the 2 mm-inset guess from dimensions.json, not measured.')

# Female headers the P4 plugs into, under its two pin rows.
board = p4('P4Board').BoundBox
for side, x in (('Left', board.XMin + 1.61), ('Right', board.XMax - 1.61)):
    add('FemaleHeader' + side, Part.makeBox(2.54, 50.8, 8.5, V(x - 1.27, board.YMin + 3.25, pb.ZMax)),
        (.10, .10, .10), 'Electronics')

# Camera: Arducam IMX519 in the Pi camera format, lens toward the front panel.
cam = p4('CameraPCB')
cb = cam.BoundBox
lens = p4('CameraLensAllowance').BoundBox
lx, ly = lens.Center.x, lens.Center.y
add('CameraPCB', cam, (.10, .35, .20), 'Electronics', note='Approximate: 25 x 24 mm board, real hole pattern.')
add('CameraLens', Part.makeBox(8.5, 8.5, 3.5, V(lx - 4.25, ly - 4.25, cb.ZMax))
    .fuse(cyl(lx, ly, cb.ZMax + 3.5, 7.5, 2.5)), (.12, .12, .12), 'Electronics',
    note='Approximate autofocus lens block.')
add('CameraFPC', Part.makeBox(21, 5.5, 2.5, V(cb.Center.x - 10.5, cb.YMin, cb.ZMin - 2.5)),
    (.85, .85, .80), 'Electronics', note='15-pin ribbon connector on the back, bottom edge.')

# Space reservations from build.py, hidden by default.
for name in ('CSIRibbonAllowance', 'PowerPlugAllowance', 'ButtonAllowance', 'ButtonCap', 'LEDAllowance'):
    add(name, p4(name), (.85, .75, .35), 'Allowances', 40, visible=False)

App.closeDocument(src.Name)
DOC.recompute()
# Where OpenAssembly.FCMacro puts the Waveshare STEP: rotated so USB-C points down.
meta = dict(board_translate=[board.XMax, board.YMin + 71.05, board.ZMax], board_rotate_z=180)
(OUT / 'assembly-board-pose.json').write_text(json.dumps(meta) + '\n')
DOC.saveAs(str(OUT / 'FullAssembly.FCStd'))
print('ASSEMBLY_OK', len(DOC.Objects), 'objects')
