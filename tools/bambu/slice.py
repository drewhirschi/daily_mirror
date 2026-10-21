#!/usr/bin/env python3
"""Slice STLs for the A1 with Bambu Studio's CLI, no GUI.

The CLI does not follow the `inherits` chain of system profiles, so this
flattens machine/process/filament profiles first. Usage:
  tools/bambu/slice.py out.gcode.3mf part.stl [more.stl ...] [--process "0.20mm Standard @BBL A1"]
"""
import argparse, json, shutil, subprocess, sys, tempfile
from pathlib import Path

PROFILES = Path('/opt/bambustudio-bin/resources/profiles/BBL')

def flatten(kind, name):
    data = json.loads((PROFILES/kind/f'{name}.json').read_text())
    parent = data.pop('inherits', None)
    merged = flatten(kind, parent) if parent else {}
    merged.update(data)
    return merged

def write(tmp, kind, name, overrides=None):
    data = flatten(kind, name)
    data.update(overrides or {})
    data.update(name=name, inherits='', **{'from': 'system'})
    path = Path(tmp)/f'{kind}.json'
    path.write_text(json.dumps(data))
    return str(path)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('out')
    ap.add_argument('stl', nargs='+')
    ap.add_argument('--machine', default='Bambu Lab A1 0.4 nozzle')
    ap.add_argument('--process', default='0.20mm Standard @BBL A1')
    ap.add_argument('--filament', default='Bambu PLA Basic @BBL A1')
    ap.add_argument('--walls', type=int, default=4)
    ap.add_argument('--infill', default='20%')
    ap.add_argument('--bed', default='Textured PEI Plate')
    ap.add_argument('--bed-temp', type=int, help='override bed temperature (C) for every plate type')
    args = ap.parse_args()
    out = Path(args.out).resolve()
    with tempfile.TemporaryDirectory() as tmp:
        machine = write(tmp, 'machine', args.machine)
        process = write(tmp, 'process', args.process,
                        {'wall_loops': str(args.walls), 'sparse_infill_density': args.infill})
        temps = {}
        if args.bed_temp:
            for k in ('hot_plate_temp', 'textured_plate_temp', 'cool_plate_temp', 'eng_plate_temp', 'supertack_plate_temp'):
                temps[k] = [str(args.bed_temp)]
                temps[k + '_initial_layer'] = [str(args.bed_temp)]
        filament = write(tmp, 'filament', args.filament, temps)
        cmd = ['bambu-studio', '--arrange', '1', '--orient', '0', '--slice', '0',
               '--curr-bed-type', args.bed,
               '--load-settings', f'{machine};{process}', '--load-filaments', filament,
               '--export-3mf', out.name, '--outputdir', tmp,
               *[str(Path(s).resolve()) for s in args.stl]]
        r = subprocess.run(cmd, cwd=tmp, capture_output=True, text=True)
        result = Path(tmp)/'result.json'
        made = Path(tmp)/out.name
        if r.returncode or not made.exists():
            sys.exit(r.stdout[-3000:] + r.stderr[-3000:])
        shutil.move(made, out)
        res = json.loads(result.read_text()) if result.exists() else {}
    for plate in res.get('sliced_plates', []):
        grams = sum(f.get('total_used_g', 0) for f in plate.get('filaments', []))
        print(f"plate {plate['id']}: {plate['main_predication']/3600:.1f} h, {grams:.0f} g")
    print('wrote', out)

if __name__ == '__main__':
    main()
