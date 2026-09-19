"""Reopen saved CAD, STEP and oriented STL; independently check export integrity."""
from pathlib import Path
import json
import FreeCAD as App
import Part
import Mesh
ROOT=Path(__file__).resolve().parent/'output'
doc=App.openDocument(str(ROOT/'DailyMirror-Rail-Enclosure-v2.FCStd'))
records=json.loads((ROOT/'manifest.json').read_text())
for rec in records:
    name=rec['name']
    shape=doc.getObject(name).Shape
    step=Part.read(str(ROOT/(name+'.step')))
    mesh=Mesh.Mesh(str(ROOT/(name+'.stl')))
    assert shape.isValid() and step.isValid() and mesh.isSolid(),name
    assert len(step.Solids)==1,name
    assert abs(shape.Volume-step.Volume)<.01,name
    assert abs(abs(mesh.Volume)-shape.Volume)/shape.Volume<.005,name
    assert mesh.BoundBox.ZMin >= -.0001,name
    assert all(abs(a-b)<.01 for a,b in zip(rec['print_size_mm'],
        (mesh.BoundBox.XLength,mesh.BoundBox.YLength,mesh.BoundBox.ZLength))),name
    print('ROUNDTRIP_OK',name)
App.closeDocument(doc.Name)
