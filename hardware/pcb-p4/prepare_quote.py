"""Export the A1 placement study for budgetary quotation, never production."""
import collections
import csv
import json
import os
from pathlib import Path
import subprocess
import zipfile
import pcbnew

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'quotes' / '2026-09-14'
OUT.mkdir(parents=True, exist_ok=True)
base = json.loads((ROOT / 'parts.json').read_text())
parts = json.loads((ROOT / 'candidate-parts.json').read_text())
passives = {'R1':'RC0603FR-071KL','R2':'RC0603FR-071KL','R3':'RC0603FR-071KL',
 'R4':'RC0603FR-0710KL','R5':'RC0603FR-07100KL','R6':'RC0603FR-07100KL',
 'R7':'RC0603FR-07100KL','C1':'GRM188R71C104KA01D'}
for p in base:
    if p['ref'].startswith('TP'): continue
    p['mpn'] = passives.get(p['ref'], '')
    p['manufacturer'] = 'Murata' if p['ref']=='C1' else ('Yageo' if p['ref'].startswith('R') else '')
    p['note'] = 'MPN proposed for quotation only; verify before release.'
    parts.append(p)
for p in parts:
    if p['ref'].startswith('SW'):
        p.update(mpn='B3F-1000', manufacturer='Omron / Aratas', note='Quotation candidate only. Verify switch footprint, height and supply before release.')
    if p['mpn'].startswith('TBD'): p['mpn']=''
    if not p['mpn']:
        p['note'] += ' NO FINAL ORDERABLE MPN. Do not omit from total without marking the quote incomplete.'
groups = {}
for p in parts:
    key=(p['mpn'] or 'UNRESOLVED '+p['ref'], p['footprint'])
    if key not in groups: groups[key] = dict(p, refs=[], qty=0)
    groups[key]['refs'].append(p['ref'])
    groups[key]['qty'] += 1
rows = list(groups.values())
for r in rows: r['refs'].sort(key=lambda x:(x.rstrip('0123456789'),int(''.join(filter(str.isdigit,x)))))
rows.sort(key=lambda r: (not r['refs'][0].startswith('U'), r['refs'][0]))
known = {'ESP32-P4NRW32X':'C54540373','ESP32-C6-MINI-1-N4':'C5736265',
 'TUSB320IRWBR':'C80170','XPGDWT-U1-0000-00J3E':'C17260425',
 'TPS62132RGTR':'C81563','2005280150':'C6050043','ABM8-40.000MHZ-10-1-U-T':'C1986574'}
for r in rows: r['lcsc']=known.get(r['mpn'],'')
(OUT/'parts-grouped.json').write_text(json.dumps(rows,indent=2)+'\n')
with (OUT/'DRAFT-BOM-JLCPCB.csv').open('w',newline='') as f:
    w=csv.writer(f);w.writerow(['Comment','Designator','Footprint','LCSC Part #','Manufacturer Part Number','Quantity','Status'])
    for r in rows: w.writerow([r['value'],','.join(r['refs']),r['footprint'],r['lcsc'],r['mpn'],r['qty'],'BUDGET ONLY; unqualified design'])
with (OUT/'DRAFT-BOM-PCBWay.csv').open('w',newline='') as f:
    w=csv.writer(f);w.writerow(['Item','Designator','Quantity per PCB','Manufacturer','Manufacturer Part Number','Description','Package','Supplier link','Notes'])
    for i,r in enumerate(rows,1):w.writerow([i,','.join(r['refs']),r['qty'],r['manufacturer'],r['mpn'],r['value'],r['footprint'],r.get('source',''),r['note']])
board=pcbnew.LoadBoard(str(ROOT/'daily-mirror-p4.kicad_pcb'))
assert board.Tracks().size()==0, 'Re-evaluate quote-only export after routing changes.'
warning=pcbnew.PCB_TEXT(board)
warning.SetText('DRAFT QUOTE ONLY\nUNROUTED - DO NOT BUILD')
warning.SetLayer(pcbnew.F_SilkS)
warning.SetPosition(pcbnew.VECTOR2I(pcbnew.FromMM(150),pcbnew.FromMM(152)))
warning.SetTextSize(pcbnew.VECTOR2I(pcbnew.FromMM(1.5),pcbnew.FromMM(1.5)))
board.Add(warning)
pcbnew.SaveBoard(str(OUT/'DRAFT-DO-NOT-BUILD.kicad_pcb'),board)
purchase_refs={p['ref'] for p in parts}
counts=collections.Counter()
with (OUT/'DRAFT-CPL-JLCPCB.csv').open('w',newline='') as f:
    w=csv.writer(f);w.writerow(['Designator','Mid X','Mid Y','Layer','Rotation'])
    for fp in sorted(board.GetFootprints(),key=lambda fp:fp.GetReference()):
        if fp.GetReference() not in purchase_refs:continue
        pos=fp.GetPosition()
        w.writerow([fp.GetReference(),round(pcbnew.ToMM(pos.x),4),round(-pcbnew.ToMM(pos.y),4),'top' if fp.GetLayer()==pcbnew.F_Cu else 'bottom',fp.GetOrientationDegrees()])
        for pad in fp.Pads():
            if pad.GetAttribute()==pcbnew.PAD_ATTRIB_SMD:counts['SMT pad apertures']+=1
            elif pad.GetAttribute()==pcbnew.PAD_ATTRIB_PTH:counts['Plated through-hole pads']+=1
counts['Purchasable placements']=len(parts)
counts['Unique BOM lines']=len(rows)
counts['Unresolved MPN lines']=sum(not r['mpn'] for r in rows)
(OUT/'quote-counts.json').write_text(json.dumps(counts,indent=2)+'\n')
readme='''DAILY MIRROR A1 — BUDGETARY QUOTE ONLY — DO NOT MANUFACTURE

This is an UNROUTED placement study. It will not operate if manufactured.
Use only to estimate fabrication/assembly and review component availability.
No purchase, production release, component reservation or paid engineering work is authorized.

Please quote quantities 1 and 10 assembled boards. If MOQ > 1, state the minimum
batch and total price separately. Destination: United States, ZIP 84003.
Candidate specification: 100 x 100 mm, 4 copper layers, 1.6 mm FR-4, green mask,
white silkscreen, ENIG, 1 oz outer / 0.5 oz inner copper, one-side SMT plus THT.
Use a standard 4-layer stackup and confirm controlled-impedance capability.
No blind/buried vias planned. Final MIPI routing, power and thermal design pending.

U1 is ESP32-P4NRW32X, QFN104, 0.35 mm pitch. Do not silently substitute old
ESP32-P4NRW32; pin 54 and power circuitry differ. Quote X-ray inspection.
The BOM contains selected candidates and explicit unresolved entries. It is not
a complete released electrical design and will change as circuitry is completed.
Test pads TP1-TP6 and holes H1-H4 are bare PCB features, excluded from purchase BOM.
RGB LED D1, service header J3, crystal load capacitors C27/C28 need final MPNs.
Switch B3F-1000 and original-control passive MPNs are quote candidates only.
Perimeter LEDs are eight white Cree devices; the Temu RGB strips are excluded.

Please separate PCB fabrication, setup/stencil/fixtures, assembly, components,
overage/reel minimums, external sourcing fees, inspection, shipping and taxes.
State stock, lead times, unavailable parts and proposed alternates explicitly.
Do not treat unmatched BOM rows as zero-cost parts or omit them from a total.
Camera module/ribbon, enclosure/diffuser, USB power adapter/cable, programming,
functional test development and regulatory testing are outside this PCB quote.

The Gerbers represent pads and outline only; there are no signal traces.
The CPL uses the board origin, mm, top side. Rotations and component centers are
unverified for the assembler and must be reviewed before a production release.
The KiCad source project is unchanged; a separate copy carries a draft warning.
'''
(OUT/'README-QUOTE-ONLY.txt').write_text(readme)
gerbers=OUT/'gerbers-DRAFT'
gerbers.mkdir(exist_ok=True)
env=dict(os.environ,XDG_CACHE_HOME='/tmp/daily-mirror-kicad-cache')
subprocess.run(['kicad-cli','pcb','export','gerbers','--layers','F.Cu,In1.Cu,In2.Cu,B.Cu,F.Paste,B.Paste,F.Mask,B.Mask,F.Silkscreen,B.Silkscreen,Edge.Cuts','--output',str(gerbers),str(OUT/'DRAFT-DO-NOT-BUILD.kicad_pcb')],env=env,check=True)
subprocess.run(['kicad-cli','pcb','export','drill','--output',str(gerbers),str(OUT/'DRAFT-DO-NOT-BUILD.kicad_pcb')],env=env,check=True)
with zipfile.ZipFile(OUT/'DRAFT-GERBERS-QUOTE-ONLY-DO-NOT-BUILD.zip','w',zipfile.ZIP_DEFLATED) as z:
    for path in sorted(gerbers.iterdir()):z.write(path,path.name)
    z.write(OUT/'README-QUOTE-ONLY.txt','README-QUOTE-ONLY.txt')
with zipfile.ZipFile(OUT/'DRAFT-RFQ-PACKAGE-DO-NOT-BUILD.zip','w',zipfile.ZIP_DEFLATED) as z:
    for name in ['README-QUOTE-ONLY.txt','DRAFT-BOM-PCBWay.csv','DRAFT-BOM-JLCPCB.csv','DRAFT-CPL-JLCPCB.csv','DRAFT-GERBERS-QUOTE-ONLY-DO-NOT-BUILD.zip']:z.write(OUT/name,name)
    z.write(ROOT/'output'/'board-3d.png','board-3d-placement-study.png')
print(json.dumps(counts,indent=2))
print(OUT)
