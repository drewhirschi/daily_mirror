#!/usr/bin/env python3
"""Rebuild the initial study in a NEW directory; never overwrite interactive edits.

Usage: python build_starter.py --output /tmp/daily-mirror-pcb-study
Requires KiCad 10's pcbnew Python module. Local libraries are the design inputs.
"""
import argparse
import json
from pathlib import Path
import shutil
import uuid

HERE = Path(__file__).resolve().parent
NAME = "daily-mirror-p4"
NS = uuid.UUID("de2f06a0-5045-4e7d-bb56-1e2d6c78f754")


def uid(key):
    return str(uuid.uuid5(NS, key))


def quoted(value):
    return json.dumps(str(value))


def block(text, name):
    start = text.index('(symbol "' + name + '"')
    depth, string, escape = 0, False, False
    for i in range(start, len(text)):
        c = text[i]
        if escape:
            escape = False
        elif string and c == "\\":
            escape = True
        elif c == '"':
            string = not string
        elif not string:
            depth += (c == "(") - (c == ")")
            if depth == 0:
                return text[start:i+1]
    raise ValueError(name)


def build(out):
    import pcbnew as pcb

    if (out / f"{NAME}.kicad_sch").exists():
        raise SystemExit("Destination already has a design; choose a new output directory.")
    out.mkdir(parents=True, exist_ok=True)
    if out != HERE:
        shutil.copytree(HERE / "libraries", out / "libraries", dirs_exist_ok=True)
    root_id = uid("schematic")
    lib = (out / "libraries/DailyMirror.kicad_sym").read_text()
    names = ["R", "C", "SW_Push", "TestPoint", "RGB_B_G_A_R"]
    libs = [block(lib, n).replace(f'(symbol "{n}"', f'(symbol "DailyMirror:{n}"', 1) for n in names]
    items, parts = [], []

    def note(text, x, y, size=1.5):
        items.append(f'(text {quoted(text)} (at {x} {y} 0) (effects (font (size {size} {size})) (justify left top)) (uuid {uid(text)}))')

    def rect(x1, y1, x2, y2):
        items.append(f'(rectangle (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.254) (type default)) (fill (type none)) (uuid {uid(str((x1,y1,x2,y2)))}))')

    def wire(a, b):
        items.append(f'(wire (pts (xy {a[0]} {a[1]}) (xy {b[0]} {b[1]})) (stroke (width 0.254) (type solid)) (uuid {uid(str((a,b)))}))')

    def label(net, at, angle=0):
        items.append(f'(label {quoted(net)} (at {at[0]} {at[1]} {angle}) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid {uid(str((net,at)))}))')

    def part(kind, ref, value, pos, nets, footprint, board_pos, angle=0):
        x, y = pos
        symbol_id = uid(ref)
        rx, ry, vx, vy = x+3.81, y-1.27, x+3.81, y+1.27
        if angle == 90:
            rx, ry, vx, vy = x-3.81, y-3.81, x-3.81, y+3.81
        if ref == 'D1':
            rx, ry, vx, vy = x-7.62, y-17.78, x-7.62, y+17.78
        items.append(f'''(symbol (lib_id "DailyMirror:{kind}") (at {x} {y} {angle})
          (unit 1) (in_bom yes) (on_board yes) (dnp no) (uuid {symbol_id})
          (property "Reference" "{ref}" (at {rx} {ry} {angle}) (effects (font (size 1.27 1.27)) (justify left)))
          (property "Value" {quoted(value)} (at {vx} {vy} {angle}) (effects (font (size 1.27 1.27)) (justify left)))
          (property "Footprint" "DailyMirror:{footprint}" (at {x} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))
          (instances (project "{NAME}" (path "/{root_id}" (reference "{ref}") (unit 1)))))''')
        parts.append(dict(ref=ref, value=value, footprint=footprint, nets=nets, pos=board_pos, uuid=symbol_id))

    note("DAILY MIRROR / CUSTOM CAMERA BOARD", 20, 16, 3)
    note("Rev A0: architecture + controls. Processor, power, camera and illumination circuits remain to be designed.", 20, 23)
    note("Requirements: existing 16 MP IMX519, USB-C wall power, Wi-Fi + phone pairing, new enclosure.", 20, 29)
    for x, y, w, h, title, content in [
        (20,42,76,39,"USB-C / POWER", "CC current detection or PD\nProtection + 3.3 V regulator\nP4 core / PHY / camera rails\nPower budget depends on lights"),
        (116,42,91,39,"PROCESSOR / P4 UNRESOLVED", "Current P4 ISP: 1920 px max width\nIMX519 full frame: 4656 px wide\nRaw bypass needs proof\nSelect processor after camera test"),
        (227,42,85,39,"ESP32-C6 / RADIO", "Wi-Fi + Bluetooth LE\nSDIO link to P4 / ESP-Hosted\nBLE provisioning on phone\nAntenna clearance + test access"),
        (20,97,76,39,"WHITE PHOTO LIGHTING", "White-only emitters / parts TBD\nDriver + current regulation TBD\nSnap-in diffuser + thermal path\nDimensions / pulse rating TBD"),
        (116,97,91,39,"IMX519 CAMERA / REQUIRED", "MIPI CSI-2, two data lanes\nI2C / autofocus / power control\nFFC pinout + contact side TBD\nFull-resolution driver work needed"),
        (227,97,85,39,"CONTROLS / DRAWN BELOW", "4-pin common-anode RGB LED\nOne resistor per color (three)\nCapture / pairing button\nTest pads for future processor")]:
        rect(x,y,x+w,y+h)
        note(title,x+3,y+3,1.8)
        note(content,x+3,y+11,1.4)
    note("Power",99,57,1.2)
    note("SDIO",209,57,1.2)
    note("CSI-2 + control",138,86,1.2)
    note("Boxes above describe planned functions; they are not connected component symbols.",20,143,1.3)
    note("RGB STATUS / ACTIVE LOW",20,158,2)
    note("Documented rpi2 mapping: pin 1 blue, 2 green, 3 common +, 4 red. Physical orientation still needs confirmation.",20,165,1.2)

    # Pins on the custom LED are left B/G/R and right common anode.
    part("RGB_B_G_A_R","D1","RGB common anode",(116.84,198.12),
         {"1":"LED_B_K","2":"LED_G_K","3":"+3V3","4":"LED_R_K"},"LED_D5.0mm-4_RGB_Wide_Pins",(143,120))
    for index, color in enumerate(["B","G","R"]):
        y=187.96 + 10.16*index
        wire((106.68,y),(97.79,y))
        label(f"LED_{color}_K",(99.06,y))
        # A 90-degree resistor has pin 1 at x-3.81 and pin 2 at x+3.81.
        part("R",f"R{index+1}","1k (start)",(93.98,y),
             {"1":f"STATUS_{color}_N","2":f"LED_{color}_K"},"R_0603_1608Metric",(143+3*index,126),angle=90)
        wire((90.17,y),(68.58,y)); label(f"STATUS_{color}_N",(68.58,y))
    wire((127,198.12),(133.35,198.12)); label("+3V3",(133.35,198.12))
    note("GPIO low = color on. Resistors initially limit each channel to <=3.3 mA.\nTune brightness using actual forward voltages. No resistor on the common leg.",20,221,1.25)

    note("BOOT-OFF PULLUPS",170,178,1.6)
    for index,color in enumerate(["B","G","R"]):
        x=175.26+22.86*index
        part("R",f"R{index+5}","100k",(x,198.12),
             {"1":"+3V3","2":f"STATUS_{color}_N"},"R_0603_1608Metric",(143+3*index,130))
        wire((x,194.31),(x,190.5));label("+3V3",(x,190.5))
        wire((x,201.93),(x,208.28));label(f"STATUS_{color}_N",(x,208.28))

    note("CAPTURE / PAIRING BUTTON",258,158,2)
    part("SW_Push","SW1","Momentary NO",(281.94,203.2),
         {"1":"USER_BUTTON_N","2":"GND"},"SW_PUSH_6mm_H4.3mm",(158,117))
    wire((276.86,203.2),(266.7,203.2));label("USER_BUTTON_N",(246.38,203.2))
    wire((246.38,203.2),(266.7,203.2))
    wire((287.02,203.2),(297.18,203.2));label("GND",(297.18,203.2))
    part("R","R4","10k",(266.7,185.42),
         {"1":"+3V3","2":"USER_BUTTON_N"},"R_0603_1608Metric",(157,126))
    wire((266.7,181.61),(266.7,175.26));label("+3V3",(266.7,175.26))
    wire((266.7,189.23),(266.7,203.2))
    part("C","C1","100nF",(266.7,215.9),
         {"1":"USER_BUTTON_N","2":"GND"},"C_0603_1608Metric",(160,126))
    wire((266.7,212.09),(266.7,203.2));wire((266.7,219.71),(266.7,226.06));label("GND",(266.7,226.06))
    items.append(f'(junction (at 266.7 203.2) (diameter 0) (color 0 0 0 0) (uuid {uid("button-junction")}))')
    note("Press pulls input low. R4/C1 = 1 ms RC.\nFirmware debounce ~20 ms; long press enters pairing.\nReset/BOOT service access belongs in the P4 circuit.",258,236,1.25)
    note("INTEGRATION TEST PADS / NO PROCESSOR ASSIGNED YET",20,242,1.6)
    for i,net in enumerate(["+3V3","GND","STATUS_B_N","STATUS_G_N","STATUS_R_N","USER_BUTTON_N"]):
        x=25.4+35.56*i
        part("TestPoint",f"TP{i+1}",net,(x,256.54),{"1":net},"TestPoint_Pad_D1.5mm",(137+5*i,134))
        wire((x,256.54),(x,264.16));label(net,(x,264.16))
    sch=f'''(kicad_sch (version 20250114) (generator "daily_mirror_starter")
      (uuid {root_id}) (paper "A3")
      (title_block (title "Daily Mirror - architecture and controls") (date "2026-09-14") (rev "A0 - STUDY")
        (company "Daily Mirror") (comment 1 "NOT FOR FABRICATION - main circuits and routing incomplete"))
      (lib_symbols {''.join(libs)}) {''.join(items)}
      (sheet_instances (path "/" (page "1"))) (embedded_fonts no))'''
    (out/f"{NAME}.kicad_sch").write_text(sch)
    (out/"sym-lib-table").write_text('(sym_lib_table (version 7) (lib (name "DailyMirror") (type "KiCad") (uri "${KIPRJMOD}/libraries/DailyMirror.kicad_sym") (options "") (descr "Project controls symbols")))\n')
    (out/"fp-lib-table").write_text('(fp_lib_table (version 7) (lib (name "DailyMirror") (type "KiCad") (uri "${KIPRJMOD}/libraries/DailyMirror.pretty") (options "") (descr "Vendored KiCad candidate footprints")))\n')

    board=pcb.BOARD()
    board.SetCopperLayerCount(4)
    mm=lambda v:pcb.FromMM(v)
    point=lambda x,y:pcb.VECTOR2I(mm(x),mm(y))
    nets={n:pcb.NETINFO_ITEM(board,'/'+n) for p in parts for n in p['nets'].values()}
    for n in nets.values():board.Add(n)
    for p in parts:
        fp=pcb.FootprintLoad(str(out/'libraries/DailyMirror.pretty'),p['footprint'])
        if fp is None:raise ValueError(p['footprint'])
        fp.SetReference(p['ref']);fp.SetValue(p['value'])
        fp.SetFPID(pcb.LIB_ID('DailyMirror',p['footprint']))
        fp.SetPosition(point(*p['pos']))
        fp.SetPath(pcb.KIID_PATH('/'+root_id+'/'+p['uuid']))
        fp.Reference().SetTextSize(point(0.8,0.8))
        fp.Reference().SetTextThickness(mm(0.12))
        fp.Value().SetVisible(False)
        for pad in fp.Pads():pad.SetNet(nets[p['nets'][pad.GetNumber()]])
        board.Add(fp)

    def line(a,b,layer,width=0.15):
        s=pcb.PCB_SHAPE();s.SetShape(pcb.SHAPE_T_SEGMENT);s.SetStart(point(*a));s.SetEnd(point(*b));s.SetLayer(layer);s.SetWidth(mm(width));board.Add(s)

    def box(x,y,w,h,layer=pcb.Dwgs_User):
        for a,b in [((x,y),(x+w,y)),((x+w,y),(x+w,y+h)),((x+w,y+h),(x,y+h)),((x,y+h),(x,y))]:line(a,b,layer)

    def text(value,x,y,size=1,layer=pcb.Dwgs_User):
        t=pcb.PCB_TEXT(board);t.SetText(value);t.SetPosition(point(x,y));t.SetTextSize(point(size,size));t.SetTextThickness(mm(size*0.14));t.SetLayer(layer);board.Add(t)

    box(100,100,100,100,pcb.Edge_Cuts)
    text("DAILY MIRROR / A0 STUDY",150,106,1.5,pcb.F_SilkS)
    text("PLACEMENT ONLY - NOT FOR FABRICATION",150,194,1,pcb.F_SilkS)
    for x,y in [(104,104),(196,104),(104,196),(196,196)]:
        fp=pcb.FootprintLoad(str(out/'libraries/DailyMirror.pretty'),'MountingHole_3.2mm_M3')
        fp.SetReference('H'+str(len([f for f in board.GetFootprints() if f.GetReference().startswith('H')])+1))
        fp.SetPosition(point(x,y));fp.SetAttributes(fp.GetAttributes()|pcb.FP_BOARD_ONLY|pcb.FP_EXCLUDE_FROM_BOM|pcb.FP_EXCLUDE_FROM_POS_FILES)
        fp.Reference().SetVisible(False);fp.Value().SetVisible(False);board.Add(fp)
    box(131,116,40,23);text("STATUS + BUTTON",151,137.5,0.8)
    box(137,143,26,25);text("IMX519 on faceplate\nLens / cable clearance\nFinal mounting TBD",150,155,1)
    box(131,174,22,13);text("P4 + memory\nSupport parts TBD",142,180.5,0.9)
    box(156,174,22,13);text("C6 + antenna\nEdge placement TBD",167,180.5,0.9)
    box(111,174,17,13);text("USB-C\nPower / driver",119.5,180.5,0.85)
    box(165,151,14,12);text("CSI connector\nPinout TBD",172,157,0.7)
    # These are mechanical drawing guides, not footprints or electrical pads.
    for x,y,w,h,label_text in [(122,108,56,8,"TOP LIGHT / SIZE TBD"),(122,188,56,5,"BOTTOM LIGHT / SIZE TBD"),(107,125,12,45,"LEFT\nLIGHT\nTBD"),(181,125,12,45,"RIGHT\nLIGHT\nTBD")]:
        box(x,y,w,h);text(label_text,x+w/2,y+h/2,0.85)
    text("100 x 100 mm provisional outline; new enclosure must follow final board.",150,203,1)
    pcb.SaveBoard(str(out/f'{NAME}.kicad_pcb'),board)
    pro={
        'meta':{'filename':f'{NAME}.kicad_pro','version':1},
        'board':{'design_settings':{'defaults':{},'rules':{'min_clearance':0.15,'min_track_width':0.15},'drc_exclusions':[]}},
        'net_settings':{'meta':{'version':3},'classes':[{'name':'Default','description':'Provisional; fabricator stackup pending','clearance':0.2,'track_width':0.25,'via_diameter':0.6,'via_drill':0.3,'diff_pair_width':0.2,'diff_pair_gap':0.2,'diff_pair_via_gap':0.25}]},
        'text_variables':{'DESIGN_STATUS':'STUDY ONLY - NOT FOR FABRICATION'},
        'libraries':{'pinned_symbol_libs':['DailyMirror'],'pinned_footprint_libs':['DailyMirror']},
        'sheets':[[root_id,'']]
    }
    (out/f'{NAME}.kicad_pro').write_text(json.dumps(pro,indent=2)+'\n')
    (out/'parts.json').write_text(json.dumps(parts,indent=2)+'\n')
    print('Created editable study:',out)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    build(parser.parse_args().output.resolve())
