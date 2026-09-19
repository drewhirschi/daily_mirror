"""Bundle source, printable parts and guide; omit scratch and FreeCAD backups."""
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
ROOT=Path(__file__).resolve().parent
OUT=ROOT/'output'
target=OUT/'DailyMirror-v2-print-pack.zip'
files=[ROOT/p for p in ['README.md','dimensions.json','build.py','build.sh','verify.py',
                       'render.py','viewer-template.html','OpenModel.FCMacro','package.py']]
files += list((ROOT/'reference').glob('*.pdf'))
files += [p for p in OUT.iterdir() if p.suffix in {'.stl','.step','.FCStd','.html','.png','.json'}]
with ZipFile(target,'w',ZIP_DEFLATED) as z:
    for p in sorted(files):
        z.write(p,Path('DailyMirror-v2')/p.relative_to(ROOT))
with ZipFile(target) as z:
    assert z.testzip() is None
    print('PACKAGE_OK',len(z.namelist()),'files;',target.stat().st_size,'bytes')
