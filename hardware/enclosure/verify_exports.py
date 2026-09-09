"""Round-trip saved CAD and print exports using FreeCADCmd."""
from pathlib import Path
import json
import FreeCAD as App
import Part
import Mesh
root=Path(__file__).resolve().parent/'output'
doc=App.openDocument(str(root/'DailyMirrorEnclosure.FCStd'))
for name in ['Base','Lid','CameraAdapter']:
    original=doc.getObject(name).Shape
    recovered=Part.read(str(root/(name+'.step')))
    mesh=Mesh.Mesh(str(root/(name+'.stl')))
    assert original.isValid() and recovered.isValid() and mesh.isSolid(),name
    assert len(recovered.Solids)==1,name
    assert abs(original.Volume-recovered.Volume)<.01,name
    print('ROUNDTRIP_OK',name)
App.closeDocument(doc.Name)
