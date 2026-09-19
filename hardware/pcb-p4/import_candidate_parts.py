#!/usr/bin/env python3
"""Build the A1 component study in a NEW directory, preserving interactive edits.

Inputs: official KiCad 10.0.6 libraries and pinned Espressif files downloaded to
/tmp/dm-parts-research. This imports real CAD assets, not a finished circuit.
Run: python import_candidate_parts.py --output /tmp/daily-mirror-a1
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys

from build_starter import uid, quoted, block

HERE = Path(__file__).resolve().parent
STOCK = Path('/home/drew/.local/share/kicad/10.0/official-10.0.6')
VENDOR = Path('/tmp/dm-parts-research')
ESP_SHA = 'dd76561812ab300351234ba6e0ec1295641796f0'
NAME = 'daily-mirror-p4'


def children(s):
    """Top-level child expressions, preserving exact source formatting."""
    result, depth, quoted_string, escape, start = [], 0, False, False, None
    for i, c in enumerate(s):
        if escape:
            escape = False
        elif quoted_string and c == '\\':
            escape = True
        elif c == '"':
            quoted_string = not quoted_string
        elif not quoted_string:
            if c == '(':
                depth += 1
                if depth == 2:
                    start = i
            elif c == ')':
                if depth == 2:
                    result.append(s[start:i+1])
                depth -= 1
    return result


def flatten(source, name, dest):
    s = block(source, name)
    parent = re.search(r'\(extends "([^"]+)"\)', s)
    if parent:
        base = flatten(source, parent[1], dest)
        props = {re.match(r'\(property "([^"]+)"', c)[1]: c
                 for c in children(s) if c.startswith('(property ')}
        items = []
        for c in children(base):
            key = re.match(r'\(property "([^"]+)"', c)
            items.append(props.pop(key[1], c) if key else c)
        items += list(props.values())
        return '(symbol '+quoted(dest)+'\n'+'\n'.join(items)+'\n)'
    return s.replace('(symbol "'+name+'"', '(symbol "'+dest+'"', 1).replace(
        '(symbol "'+name+'_', '(symbol "'+dest+'_')


def build(out):
    import pcbnew as pcb
    if out.exists():
        raise SystemExit('Choose a new output directory; existing designs are never overwritten.')
    out.mkdir(parents=True)
    shutil.copytree(HERE/'libraries', out/'libraries')
    baseline = HERE/'revisions/a0'
    if not baseline.exists():
        baseline = HERE
    for suffix in ['kicad_sch', 'kicad_pcb', 'kicad_pro']:
        shutil.copy2(baseline/f'{NAME}.{suffix}', out/f'{NAME}.{suffix}')
    for filename in ['sym-lib-table','fp-lib-table','parts.json']:
        shutil.copy2(baseline/filename, out/filename)
    libdir = out/'libraries/DailyMirror.pretty'
    modeldir = out/'libraries/3dmodels'
    symbols, imported, parts = {}, {}, []
    sys.path.append('/usr/lib/freecad/lib')
    import FreeCAD, Part

    def package_proxy(name):
        """Visible, explicitly approximate CAD envelopes for unavailable models."""
        specs = {
            'Molex_200528-0150_1x15-1MP_P1.00mm_Horizontal.step': (20.2,5.3,1.9,0,-1.5,(0.75,0.72,0.63)),
            'Texas_X2QFN-12_1.6x1.6mm_P0.4mm.step': (1.6,1.6,0.4,0,0,(0.12,0.12,0.14)),
            'VQFN-16-1EP_3x3mm_P0.5mm_EP1.68x1.68mm.step': (3,3,1,0,0,(0.12,0.12,0.14)),
            'LED_Cree-XP-G.step': (3.45,3.45,0.7,0,0,(0.9,0.87,0.76)),
        }
        if name not in specs:
            raise FileNotFoundError('No model or documented envelope for '+name)
        w,d,h,x,y,color=specs[name]
        stem=Path(name).stem+'-package-proxy'
        shape=Part.makeBox(w,d,h,FreeCAD.Vector(x-w/2,y-d/2,0.05))
        shape.exportStep(str(modeldir/(stem+'.step')))
        def box(w,d,h,x,y,z,color):
            return f'Transform {{ translation {x/2.54} {y/2.54} {z/2.54} children [ Shape {{ appearance Appearance {{ material Material {{ diffuseColor {" ".join(map(str,color))} }} }} geometry Box {{ size {w/2.54} {d/2.54} {h/2.54} }} }} ] }}'
        wrl='#VRML V2.0 utf8\n# Approximate package envelope; not manufacturer MCAD.\n'+box(w,d,h,x,y,h/2+.05,color)
        if 'Cree' in name:
            wrl+='\n'+box(2.7,2.7,.45,0,0,.975,(1,.75,.08))
        if 'Molex' in name:
            wrl+='\n'+box(18.4,1.4,.55,0,-3.35,1.75,(.18,.16,.14))
        (modeldir/(stem+'.wrl')).write_text(wrl+'\n')
        return modeldir/(stem+'.wrl')
    sheets = {
        'processor': ('Processor, radio and camera - candidate parts', 2),
        'power': ('USB-C, regulators and service - candidate parts', 3),
        'lighting': ('White illumination - candidate parts', 4),
        'support': ('Support components - preliminary quantities', 5),
    }

    def get_symbol(lib, name, alias=None):
        alias = alias or name
        if alias not in symbols:
            src = VENDOR/'Espressif.kicad_sym' if lib == 'Espressif' else STOCK/'symbols'/f'{lib}.kicad_sym'
            symbols[alias] = flatten(src.read_text(), name, alias)
        return alias

    def get_fp(identifier):
        lib, name = identifier.split(':')
        if name in imported:
            return name
        src = VENDOR/(name+'.kicad_mod') if lib == 'Espressif' else STOCK/'footprints'/(lib+'.pretty')/(name+'.kicad_mod')
        s = src.read_text()
        models = []
        generated_proxy = False
        for old in re.findall(r'\(model "([^"]+)"', s):
            if lib == 'Espressif':
                modelsrc = VENDOR/Path(old).name
            else:
                modelsrc = STOCK/'3dmodels'/old.split('}/',1)[1]
            if not modelsrc.exists() and (VENDOR/Path(old).name).exists():
                modelsrc = VENDOR/Path(old).name
            if not modelsrc.exists():
                modelsrc = package_proxy(Path(old).name)
                generated_proxy = True
            if modelsrc.parent != modeldir:
                shutil.copy2(modelsrc, modeldir/modelsrc.name)
            s = s.replace(old, '${KIPRJMOD}/libraries/3dmodels/'+modelsrc.name)
            models.append(modelsrc.name)
        (libdir/(name+'.kicad_mod')).write_text(s)
        imported[name] = dict(source=identifier, source_sha256=hashlib.sha256(src.read_bytes()).hexdigest(), models=models)
        if generated_proxy:
            imported[name]['model_status']='Locally generated package envelope because upstream model is unavailable; not exact manufacturer MCAD; use only for placement illustration'
        return name

    def add(ref, lib, sym, value, fp, group, pos, schpos, maker, mpn, source, note='', rotation=0, alias=None):
        kind = get_symbol(lib, sym, alias)
        if ref == 'U4':
            fp = 'Package_DFN_QFN:VQFN-16-1EP_3x3mm_P0.5mm_EP1.68x1.68mm'
        if not fp:
            fp = re.search(r'\(property "Footprint" "([^"]+)"', symbols[kind])[1]
        footprint = get_fp(fp)
        parts.append(dict(ref=ref, symbol=kind, value=value, footprint=footprint,
                          sheet=group, pos=pos, schpos=schpos, rotation=rotation,
                          manufacturer=maker, mpn=mpn, source=source, note=note,
                          status='Candidate; wiring and qualification pending', uuid=uid('candidate-'+ref)))

    add('U1','Espressif','ESP32-P4X','ESP32-P4NRW32X','Espressif:ESP32-P4','processor',(145,173),(111.76,104.14),
        'Espressif','ESP32-P4NRW32X','https://documentation.espressif.com/esp32-p4_datasheet_en.html',
        'Current X / rev3+ symbol. IMX519 full-resolution path unproven. 32 MB in-package PSRAM; external flash required.')
    add('U2','Espressif','ESP32-C6-MINI-1/U','ESP32-C6-MINI-1-N4','Espressif:ESP32-C6-MINI-1','processor',(185,116),(266.7,93.98),
        'Espressif','ESP32-C6-MINI-1-N4','https://www.espressif.com/sites/default/files/documentation/esp32-c6-mini-1_datasheet_en.pdf',
        'Wi-Fi/BLE companion. SDIO/ESP-Hosted integration pending. Antenna needs copper/component keepout.',alias='ESP32-C6-MINI-1')
    add('U3','Memory_Flash','W25Q128JVS','W25Q128JVSIQ',None,'processor',(160,175),(226.06,170.18),
        'Winbond','W25Q128JVSIQ','https://www.winbond.com/resource-files/w25q128jv_dtr%20revc%2003272018%20plus.pdf','16 MiB external SPI flash; boot flash voltage and strapping pending.')
    add('J2','Connector_Generic','Conn_01x15','Pi-style CSI camera - 15 pin','Connector_FFC-FPC:Molex_200528-0150_1x15-1MP_P1.00mm_Horizontal','processor',(173,150),(327.66,177.8),
        'Molex','2005280150','https://www.molex.com/en-us/products/part-detail/2005280150','Bottom-contact, 1 mm pitch. Confirm actual Arducam module/ribbon contact side and pinout before wiring.')
    add('Y1','Device','Crystal_GND24','40 MHz / 10 pF','Crystal:Crystal_SMD_3225-4Pin_3.2x2.5mm','processor',(133,173),(132.08,182.88),
        'Abracon','ABM8-40.000MHZ-10-1-U-T','https://abracon.com/Support/SPICE/Resonators/ABM8-Series%20ParameterTest%20Data_SPICE%20MODEL.pdf','Manufacturer test report gives +/-10 ppm. Ordering availability and temperature specification need confirmation; generic package model.')

    powerparts = [
        ('U4','Regulator_Switching','TPS62132','TPS62132RGTR',(120,180),(111.76,76.2),'Texas Instruments','TPS62132RGTR','https://www.ti.com/product/TPS62132','Fixed 3.3 V, 3 A main buck. Inductor/output network and load budget pending.'),
        ('U5','Regulator_Switching','TLV62569DBV','TLV62569DBVR',(134,183),(200.66,76.2),'Texas Instruments','TLV62569DBVR','https://www.ti.com/product/TLV62569','P4 core buck. Use rev3+ feedback/control circuit, not a fixed generic 1.1 V assumption.'),
        ('U6','Interface_USB','TUSB320I','TUSB320IRWBR',(119,166),(68.58,165.1),'Texas Instruments','TUSB320IRWBR','https://www.ti.com/product/TUSB320/part-details/TUSB320IRWBR','5 V Type-C source-current detection. Configure sink. Not a PD controller; gate load to advertised current. Stock availability not confirmed.'),
        ('U8','Power_Protection','USBLC6-2SC6','USBLC6-2SC6',(113,190),(157.48,165.1),'STMicroelectronics','USBLC6-2SC6','https://www.st.com/en/protection-devices/usblc6-2.html','USB data-line ESD protection; additional VBUS/CC protection design pending.'),
        ('U9','Power_Management','TPS22919DCK','TPS22919DCKR',(177,161),(251.46,165.1),'Texas Instruments','TPS22919DCKR','https://www.ti.com/product/TPS22919','Camera power switch candidate; confirm camera module supply and startup current.'),
    ]
    for ref,lib,sym,value,pos,schpos,maker,mpn,url,note in powerparts:
        add(ref,lib,sym,value,None,'power',pos,schpos,maker,mpn,url,note)
    add('J1','Connector','USB_C_Receptacle_USB2.0_16P','USB-C / 5 V','Connector_USB:USB_C_Receptacle_GCT_USB4105-xx-A_16P_TopMnt_Horizontal','power',(120,197),(35.56,76.2),
        'GCT','USB4105-GF-A','https://gct.co/connector/USB4105','Select exact stake length/order suffix with board thickness; input load protection/inrush circuitry pending.')
    add('J3','Connector_Generic','Conn_01x06','Service UART / boot','Connector_PinHeader_2.54mm:PinHeader_1x06_P2.54mm_Vertical','power',(169,180),(337.82,167.64),
        'Generic','TBD 1x06 2.54 mm header','https://gitlab.com/kicad/libraries/kicad-footprints','Service pinout not assigned; prefer production test pads in final design.')
    for ref,pos,schpos,role in [('SW2',(180,181),(287.02,68.58),'RESET'),('SW3',(191,181),(337.82,68.58),'BOOT')]:
        add(ref,'Switch','SW_Push',role,'Button_Switch_THT:SW_PUSH_6mm_H4.3mm','power',pos,schpos,
            'Generic','TBD 6 mm tactile','https://gitlab.com/kicad/libraries/kicad-footprints','Generic body; choose exact switch force/actuator height for enclosure.')
    add('U7','Driver_LED','TPS61165DBV','TPS61165DBVR',None,'lighting',(181,137),(63.5,78.74),
        'Texas Instruments','TPS61165DBVR','https://www.ti.com/product/TPS61165','Constant-current boost candidate. 1.2 A is switch rating, not available LED output current. Tentative eight-series / 100 mA for initial optical study only.')
    add('D10','Device','D_Schottky','SS16-E3/61T','Diode_SMD:D_SMA','lighting',(190,137),(127,71.12),
        'Vishay','SS16-E3/61T','https://www.vishay.com/en/product/88746/','Boost rectifier candidate; calculate peak current, losses and reverse voltage.')
    led_symbol = get_symbol('Device','LED','LED_XPG3_thermal')
    symbols[led_symbol] = symbols[led_symbol].rstrip()[:-1] + '''
      (symbol "LED_XPG3_thermal_1_1"
        (pin passive line (at 0 -7.62 90) (length 2.54)
          (name "THERMAL" (effects (font (size 1 1))))
          (number "3" (effects (font (size 1 1)))))) )'''
    # Merge the two unit-1 symbol blocks so KiCad never has ambiguous units.
    pieces = children(symbols[led_symbol]); units = [x for x in pieces if x.startswith('(symbol "LED_XPG3_thermal_1_1"')]
    pieces = [x for x in pieces if x not in units]
    pieces.append('(symbol "LED_XPG3_thermal_1_1"\n'+'\n'.join(c for u in units for c in children(u))+')')
    symbols[led_symbol] = '(symbol "LED_XPG3_thermal"\n'+'\n'.join(pieces)+')'
    led_positions = [(122,111),(159,111),(190,139),(190,162),(177,190),(145,190),(110,160),(110,130)]
    # Right-hand upper light shifted to leave room for its driver.
    led_positions[2] = (190,130)
    for i,pos in enumerate(led_positions):
        add('D'+str(i+2),'Device','LED','White 5000 K / CRI90','LED_SMD:LED_Cree-XP-G','lighting',pos,(30.48+43.18*(i%8),154.94),
            'Cree LED','XPGDWT-U1-0000-00J3E','https://downloads.cree-led.com/files/ds/x/XLamp-XPG3.pdf',
            'Plain white XP-G3. Qty eight provisional. Shared XP-G footprint and package STEP are candidates, not an exact manufacturer XP-G3 model. Thermal pad electrically isolated from LED; heat-spreading copper required.',alias=led_symbol)
    for ref,ind,foot,group,pos,schpos in [
        ('L1','XAL4020-222MEC','L_Coilcraft_XAL4020-XXX','power',(120,173),(111.76,114.3)),
        ('L2','XAL4020-222MEC','L_Coilcraft_XAL4020-XXX','power',(134,190),(200.66,114.3)),
        ('L3','XAL4040-103MEC','L_Coilcraft_XAL4040-XXX','lighting',(182,145),(101.6,73.66))]:
        add(ref,'Device','L','2.2 uH' if ref!='L3' else '10 uH','Inductor_SMD:'+foot,group,pos,schpos,
            'Coilcraft',ind,'https://www.coilcraft.com/en-us/products/power/shielded-inductors/molded-inductor/xal/xal40xx/'+ind.split('MEC')[0].lower()+'/',
            'Electrical value and current/temperature margin provisional; package imported from KiCad.')

    # Preliminary decoupling/support population: real physical package choices,
    # explicitly unconnected until the reference circuit is adapted.
    cap_specs = [
        ('100nF','GRM188R71C104KA01D','C_0603_1608Metric','16 V X7R'),
        ('1uF','GRM188R61A105KA61D','C_0603_1608Metric','10 V X5R'),
        ('10uF','GRM21BR61A106KE19L','C_0805_2012Metric','10 V X5R'),
        ('22pF','GRM1885C1H220JA01D','C_0603_1608Metric','50 V C0G'),
        ('1uF','GRM21BR71H105KA12L','C_0805_2012Metric','50 V X7R; LED boost output candidate'),
    ]
    cap_positions = [(137,165),(140,165),(143,165),(146,165),(149,165),(152,165),
        (137,181),(140,181),(143,181),(146,181),(149,181),(152,181),
        (138,169),(138,177),(152,170),(152,174),
        (174,113),(174,117),(165,169),(125,173),(125,180),(129,185),(177,137),(187,144)]
    for i,pos in enumerate(cap_positions):
        k = 0 if i<16 else [2,0,0,2,2,2,0,4][i-16]
        value,mpn,fp,note = cap_specs[k]
        add('C'+str(i+2),'Device','C',value,'Capacitor_SMD:'+fp,'support',pos,(30.48+43.18*(i%8),60.96+40.64*(i//8)),
            'Murata',mpn,'https://www.murata.com/en-us/products/capacitor/ceramiccapacitor',note+'; quantities/value/rail assignments provisional, DC-bias derating pending.')
    for i,(ref,value,mpn,pos,note) in enumerate([
        ('R8','499k','RC0603FR-07499KL',(130,181),'P4 rev3 core control network'),
        ('R9','499k','RC0603FR-07499KL',(130,189),'P4 rev3 core control network'),
        ('R10','4.02k','RC0603FR-074K02L',(155,165),'CSI REXT'),
        ('R11','4.02k','RC0603FR-074K02L',(158,165),'DSI REXT if DSI used'),
        ('R12','10k','RC0603FR-0710KL',(176,173),'Reset pull-up candidate'),
        ('R13','10k','RC0603FR-0710KL',(189,173),'Boot pull-up candidate'),
        ('R14','100k','RC0603FR-07100KL',(177,141),'LED enable default-off candidate'),
        ('R15','2 ohm','RC0805FR-072RL',(185,139),'LED current sense: 0.2 V / 2 ohm = 100 mA initial study'),
        ('R16','2.2k','RC0603FR-072K2L',(175,155),'Camera I2C pull-up candidate'),
        ('R17','2.2k','RC0603FR-072K2L',(178,155),'Camera I2C pull-up candidate'),
    ]):
        fp = 'R_0805_2012Metric' if ref=='R15' else 'R_0603_1608Metric'
        add(ref,'Device','R',value,'Resistor_SMD:'+fp,'support',pos,(30.48+43.18*(i%8),187.96+33.02*(i//8)),
            'Yageo',mpn,'https://www.yageogroup.com/component-documentation/download/specsheet/'+mpn,note+'; not wired; value and MPN ordering check pending.')
    for ref,pos,schpos,role in [('C26',(130,185),(162.56,220.98),'Core compensation'),('C27',(130,170),(205.74,220.98),'Crystal load'),('C28',(130,176),(248.92,220.98),'Crystal load')]:
        add(ref,'Device','C','22pF' if ref=='C26' else 'TUNE pF','Capacitor_SMD:C_0603_1608Metric','support',pos,schpos,
            'Murata','GRM1885C1H220JA01D' if ref=='C26' else 'TBD C0G 0603',
            'https://www.murata.com/en-us/products/capacitor/ceramiccapacitor',role+'; crystal capacitors require load/stray calculation and measurement.')

    # Space component courtyards apart; these are still placement-study locations.
    adjusted = {'U5':(131,185),'C23':(126,188),'C26':(137,185),
                'R8':(134,180),'R9':(137,188),'L2':(134,193),
                'C27':(128,170),'C28':(128,176),
                'C14':(137,168),'C15':(137,177),'C16':(153,170),'C17':(153,176),
                'R16':(176,157),'R17':(179,157),'R15':(182,141)}
    for p in parts:
        p['pos'] = adjusted.get(p['ref'],p['pos'])

    # Espressif supplies no P4 STEP in this pinned library. Create an explicitly
    # labelled body-only envelope: no invented lead arrangement or markings.
    sys.path.append('/usr/lib/freecad/lib')
    import FreeCAD, Part
    shape = Part.makeBox(10,10,0.9,FreeCAD.Vector(-5,-5,0.05))
    shape.exportStep(str(modeldir/'ESP32-P4-body-envelope.step'))
    (modeldir/'ESP32-P4-body-envelope.wrl').write_text('''#VRML V2.0 utf8
# Nominal body-only envelope, not manufacturer MCAD.
Transform { translation 0 0 0.197 children [ Shape {
 appearance Appearance { material Material { diffuseColor 0.10 0.10 0.12 } }
 geometry Box { size 3.937 3.937 0.3543 } } ] }
''')
    f = libdir/'ESP32-P4.kicad_mod'
    f.write_text(f.read_text().rstrip()[:-1]+'\n(model "${KIPRJMOD}/libraries/3dmodels/ESP32-P4-body-envelope.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))\n)')
    imported['ESP32-P4']['models'] = ['ESP32-P4-body-envelope.wrl','ESP32-P4-body-envelope.step']
    imported['ESP32-P4']['model_status'] = 'Locally generated nominal 10 x 10 x 0.9 mm body-only envelope; not manufacturer MCAD; height/pads not for mechanical signoff'

    # Use project-local footprint defaults in every imported symbol.
    for p in parts:
        s = symbols[p['symbol']]
        prop = next((c for c in children(s) if c.startswith('(property "Footprint"')),None)
        replacement = f'(property "Footprint" "DailyMirror:{p["footprint"]}" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))'
        symbols[p['symbol']] = s.replace(prop,replacement) if prop else s.rstrip()[:-1]+replacement+')'
    (out/'libraries/DailyMirrorParts.kicad_sym').write_text('(kicad_symbol_lib (version 20251024) (generator "daily_mirror_part_import")\n'+'\n'.join(symbols.values())+'\n)')
    table=(out/'sym-lib-table').read_text().rstrip()
    (out/'sym-lib-table').write_text(table[:-1]+'\n(lib (name "DailyMirrorParts") (type "KiCad") (uri "${KIPRJMOD}/libraries/DailyMirrorParts.kicad_sym") (options "") (descr "Sourced candidate parts; integration pending")))\n')

    rootid = uid('schematic')
    root = (out/f'{NAME}.kicad_sch').read_text()
    root = root.replace('Rev A0: architecture + controls. Processor, power, camera and illumination circuits remain to be designed.',
        'Rev A1: component assets imported on sheets 2-5; controls connected below. Main circuits not yet connected.')
    root = root.replace('A0 - STUDY','A1 - PARTS STUDY')
    sheetobjects = []
    for index,(key,(title,page)) in enumerate(sheets.items()):
        sheetid = uid('sheet-'+key)
        sx,sy=335.28,45.72+30.48*index
        sheetobjects.append(f'''(sheet (at {sx} {sy}) (size 63.5 17.78)
          (stroke (width 0.254) (type solid)) (fill (color 0 0 0 0)) (uuid {sheetid})
          (property "Sheetname" "{key.title()} candidates" (at {sx} {sy-1.27} 0) (effects (font (size 1.27 1.27)) (justify left bottom)))
          (property "Sheetfile" "{key}.kicad_sch" (at {sx} {sy+19.05} 0) (effects (font (size 1.27 1.27)) (justify left top)))
          (instances (project "{NAME}" (path "/{rootid}" (page "{page}")))))''')
        sheetparts = [p for p in parts if p['sheet']==key]
        defs = [symbols[n].replace('(symbol "'+n+'"','(symbol "DailyMirrorParts:'+n+'"',1) for n in dict.fromkeys(p['symbol'] for p in sheetparts)]
        items = []
        for p in sheetparts:
            x,y=p['schpos'];ref=p['ref'];kind=p['symbol']
            # Reference/value above symbol bounds; P4 is much taller than passives.
            ats=[tuple(map(float,m)) for m in re.findall(r'\(at (-?[\d.]+) (-?[\d.]+) (?:0|90|180|270)\)',symbols[kind])]
            top=max([v[1] for v in ats]+[3.81])
            py=max(28,y-top-5.08)
            if ref=='U1': py=40
            if ref=='U2': py=48
            items.append(f'''(symbol (lib_id "DailyMirrorParts:{kind}") (at {x} {y} 0) (unit 1)
              (in_bom yes) (on_board yes) (dnp no) (uuid {p['uuid']})
              (property "Reference" "{ref}" (at {x} {py} 0) (effects (font (size 1.27 1.27))))
              (property "Value" {quoted(p['value'])} (at {x} {py+2.54} 0) (effects (font (size 1.27 1.27))))
              (property "Footprint" "DailyMirror:{p['footprint']}" (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
              (property "MPN" {quoted(p['mpn'])} (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
              (property "Datasheet" {quoted(p['source'])} (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
              (property "Design status" {quoted(p['status'])} (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
              (instances (project "{NAME}" (path "/{rootid}/{sheetid}" (reference "{ref}") (unit 1)))))''')
        for text,x,y,size in [(title,20,15,2.3),('PART IMPORT / pins intentionally awaiting circuit design. This page is NOT a connected circuit.',20,23,1.5),
            ('Values, quantities and mechanical placement are provisional. See sourcing.md and candidate-parts.csv.',20,263,1.4)]:
            items.append(f'(text {quoted(text)} (at {x} {y} 0) (effects (font (size {size} {size})) (justify left top)) (uuid {uid(key+text)}))')
        sch=f'''(kicad_sch (version 20250114) (generator "daily_mirror_part_import") (uuid {uid('document-'+key)}) (paper "A3")
          (title_block (title {quoted(title)}) (rev "A1 - CANDIDATES") (comment 1 "NOT FOR FABRICATION"))
          (lib_symbols {''.join(defs)}) {''.join(items)} (embedded_fonts no))'''
        (out/f'{key}.kicad_sch').write_text(sch)
    root=root.rstrip()[:-1]+'\n'+'\n'.join(sheetobjects)+'\n)'
    (out/f'{NAME}.kicad_sch').write_text(root)
    project=json.loads((out/f'{NAME}.kicad_pro').read_text())
    project['net_settings']['classes'][0]['clearance']=0.15
    project['libraries']['pinned_symbol_libs']=['DailyMirror','DailyMirrorParts']
    (out/f'{NAME}.kicad_pro').write_text(json.dumps(project,indent=2)+'\n')

    board=pcb.LoadBoard(str(out/f'{NAME}.kicad_pcb'))
    point=lambda x,y:pcb.VECTOR2I(pcb.FromMM(x),pcb.FromMM(y))
    # Replace provisional drawing reservations with placed components and labels.
    drawings = board.Drawings()
    iterator = drawings.iterator()
    drawing_items = []
    for _ in range(len(drawings)):
        drawing_items.append(iterator.value())
        iterator.incr()
    for d in drawing_items:
        if d.GetLayer() in [pcb.Dwgs_User,pcb.F_SilkS]: board.Remove(d)
    for p in parts:
        fp=pcb.FootprintLoad(str(libdir),p['footprint'])
        assert fp is not None,p
        fp.SetReference(p['ref']);fp.SetValue(p['value'])
        fp.SetFPID(pcb.LIB_ID('DailyMirror',p['footprint']))
        fp.SetPosition(point(*p['pos']));fp.SetOrientationDegrees(p['rotation'])
        fp.SetPath(pcb.KIID_PATH('/'+rootid+'/'+uid('sheet-'+p['sheet'])+'/'+p['uuid']))
        fp.Reference().SetTextSize(point(0.65,0.65));fp.Reference().SetTextThickness(pcb.FromMM(0.1))
        fp.Value().SetVisible(False)
        for pad in fp.Pads():pad.SetNetCode(0)
        board.Add(fp)
    def text(value,x,y,size=1,layer=pcb.F_SilkS):
        t=pcb.PCB_TEXT(board);t.SetText(value);t.SetPosition(point(x,y));t.SetTextSize(point(size,size));t.SetTextThickness(pcb.FromMM(size*.14));t.SetLayer(layer);board.Add(t)
    text('DAILY MIRROR / A1 PARTS STUDY',144,105,1.15)
    text('UNROUTED - NOT FOR FABRICATION',155,196,0.85)
    text('RGB',142,115,.8);text('CAPTURE / PAIR',160,113,.7)
    text('P4 + FLASH',149,185,.75);text('5V USB-C',119,187,.7)
    text('Wi-Fi / BLE',184,123,.7);text('CAMERA CSI / 15P',174,146,.7)
    text('WHITE DRIVER',183,133.5,.7)
    text('IMX519 ON FACEPLATE\nRIBBON TO J2\nMOUNTING STILL TBD',149,152,1.1)
    text('8 WHITE LEDs / DIFFUSER ABOVE PERIMETER',150,207,1,pcb.Dwgs_User)
    for a,b in [((137,140),(162,140)),((162,140),(162,162)),((162,162),(137,162)),((137,162),(137,140))]:
        s=pcb.PCB_SHAPE();s.SetShape(pcb.SHAPE_T_SEGMENT);s.SetStart(point(*a));s.SetEnd(point(*b));s.SetLayer(pcb.Dwgs_User);s.SetWidth(pcb.FromMM(.15));board.Add(s)
    pcb.SaveBoard(str(out/f'{NAME}.kicad_pcb'),board)
    (out/'candidate-parts.json').write_text(json.dumps(parts,indent=2)+'\n')
    with (out/'candidate-parts.csv').open('w') as f:
        keys=['ref','value','manufacturer','mpn','footprint','sheet','status','source','note']
        writer=csv.DictWriter(f,fieldnames=keys,extrasaction='ignore');writer.writeheader();writer.writerows(parts)
    (out/'libraries/import-manifest.json').write_text(json.dumps({'kicad_version':'10.0.6','espressif_commit':ESP_SHA,'footprints':imported},indent=2)+'\n')
    shutil.copy2(VENDOR/'LICENSE.md',out/'libraries/Espressif-LICENSE.md')
    print(f'Imported {len(parts)} candidate placements, {len(symbols)} symbols, {len(imported)} footprints into {out}')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    build(parser.parse_args().output.resolve())
