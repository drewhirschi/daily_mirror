"""FreeCADCmd build.py. All dimensions mm; dimensions.json is the single source.

Closed tray + full lid for the Raspberry Pi 4 Model B, an Arducam IMX519 behind
the lid (lens out through a bore), and beside it a 12 mm tactile switch under a
printed cap plus a 3 mm status LED.
Axes as in enclosure-p4-open: X across, Y along (microSD end at Y=0, camera end
at Y=L), Z out from the flat back (Z=0). Seen from the front, USB-C and the
lens are on the right.

Pi frame used below ("px, py, pz"): origin at the PCB corner where the microSD
edge meets the USB-C/HDMI edge, px along the 85 mm side toward USB/Ethernet,
py along the 56 mm side toward the GPIO header, pz up from the PCB top face.
The Pi sits rotated 90 degrees: X = X0 - py, Y = Y0 + px, Z = ZP + pz.
"""
import json, math, zipfile
from pathlib import Path
import FreeCAD as App
import Part
import MeshPart

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
OUT.mkdir(exist_ok=True)
C = json.loads((ROOT / 'dimensions.json').read_text())
V = App.Vector
T, LT, H = C['wall'], C['lid_thickness'], C['total_height']

# Raspberry Pi 4 Model B, from the official mechanical drawing (see README for
# which numbers are dimensioned and which are scaled or estimated).
PI_L, PI_W, PI_R = 85.0, 56.0, 3.0
PI_T = C['pi_pcb_thickness']
PI_HOLES = [(3.5, 3.5), (61.5, 3.5), (3.5, 52.5), (61.5, 52.5)]
USBC = dict(px=11.2, w=9.0, h=3.2, over=1.2, depth=7.4)
HDMI = [dict(px=26.0, w=6.5, h=3.0, over=1.6, depth=7.6), dict(px=39.5, w=6.5, h=3.0, over=1.6, depth=7.6)]
AUDIO = dict(px=54.0, w=7.0, h=6.0, over=2.6, depth=12.0)
USB_A = [dict(py=9.0, w=13.3, h=16.0, over=3.0, depth=17.5), dict(py=27.0, w=13.3, h=16.0, over=3.0, depth=17.5)]
ETH = dict(py=45.75, w=15.8, h=13.5, over=3.0, depth=21.0)
CSI = dict(px=46.5, py=11.5, w=2.9, l=22.4, h=5.5)
DSI = dict(px=4.0, py=28.0, w=4.0, l=21.0, h=5.5)
GPIO = dict(px=32.5, py=52.5, l=50.8, w=5.08, h=8.5)
SD_PY, SD_CARD_W, SD_OVER = 28.0, 11.0, 2.5

DOC = App.newDocument('DailyMirrorPi4')
PRINT = DOC.addObject('App::DocumentObjectGroup', 'PrintableParts')
REF = DOC.addObject('App::DocumentObjectGroup', 'HardwareReferences')
FIT = DOC.addObject('App::DocumentObjectGroup', 'FitCoupons')
parts, refs, coupons = [], [], []

def box(x, y, z, w, h, d):
    return Part.makeBox(w, h, d, V(x, y, z))

def cylinder(x, y, z, diameter, height, axis=(0, 0, 1)):
    return Part.makeCylinder(diameter / 2, height, V(x, y, z), V(*axis))

def union(shapes):
    return shapes[0].multiFuse(shapes[1:]).removeSplitter() if len(shapes) > 1 else shapes[0]

def subtract(shape, cutters):
    return shape.cut(Part.makeCompound(cutters)).removeSplitter()

def roundbox(x, y, z, w, h, d, r):
    return union([box(x+r, y, z, w-2*r, h, d), box(x, y+r, z, w, h-2*r, d)] +
                 [cylinder(x+a, y+b, z, 2*r, d) for a in (r, w-r) for b in (r, h-r)])

def add(name, shape, color, group=PRINT, note='', flip=False):
    if group != REF:
        shape = shape.removeSplitter()
        assert len(shape.Solids) == 1, (name, len(shape.Solids))
    assert not shape.isNull() and shape.isValid(), name
    obj = DOC.addObject('Part::Feature', name)
    obj.Shape = shape
    obj.addProperty('App::PropertyString', 'AssemblyNote')
    obj.AssemblyNote = note
    group.addObject(obj)
    record = dict(name=name, obj=obj, color=color, note=note, flip=flip)
    (parts if group == PRINT else refs if group == REF else coupons).append(record)
    return shape

# LAYOUT ---------------------------------------------------------------------
RIM = H - LT                                    # tray rim = lid underside
W = 2*T + C['pi_port_side_gap'] + PI_W + C['pi_gpio_side_gap']
X0 = W - T - C['pi_port_side_gap']              # X of the Pi's USB-C/HDMI edge
Y0 = T + C['pi_sd_end_gap']                     # Y of the Pi's microSD edge
ZP = T + C['pi_post_height'] + PI_T             # Z of the Pi's PCB top face

def pbox(px0, px1, py0, py1, pz0, pz1):
    return box(X0-py1, Y0+px0, ZP+pz0, py1-py0, px1-px0, pz1-pz0)

pi_holes = [(X0-py, Y0+px) for px, py in PI_HOLES]
pi_end = Y0 + PI_L + max(p['over'] for p in USB_A + [ETH])     # Y of the USB/Ethernet faces

# Camera hangs under the lid in the bay past the Pi's USB end: connector edge
# toward the Pi (down on the wall), so the picture is upright.
CBW, CBL, CBT = C['camera_board_width'], C['camera_board_length'], C['camera_board_thickness']
cam_post_h = C['camera_lens_block_height'] + C['camera_lens_block_gap']
cam_x = X0 - CSI['py']                          # ribbon cannot shift sideways
cam_y0 = Y0 + PI_L + C['camera_edge_after_pi']  # connector edge
cam_y1 = cam_y0 + CBL
cam_front = RIM - cam_post_h                    # PCB face that carries the lens
cam_back = cam_front - CBT
lens_y = cam_y0 + C['camera_lens_from_connector_edge']
hy = cam_y1 - C['camera_hole_from_far_edge']
cam_holes = [(cam_x+sx*C['camera_hole_spacing_x']/2, y) for sx in (-1, 1)
             for y in (hy, hy-C['camera_hole_spacing_y'])]
L = round(cam_y1 + C['camera_end_margin'] + T, 2)
# Button: a 12 mm tactile switch on a screwed-on carrier under the lid, pressed
# through the lid by a printed cap whose flange keeps it captive.
btn_x, btn_y = C['button_x'], lens_y
cap_top = RIM - C['cap_rest_gap']               # flange top, cap at rest
cap_bottom = cap_top - C['cap_flange_thickness']
sw_top = cap_bottom + C['cap_socket_depth']     # plunger top
sw_seat = sw_top - C['switch_height']           # switch seating plane = carrier top
sw_tip = sw_seat - C['switch_terminal_length']  # terminal tips
sw_post_h = RIM - sw_seat
sw_posts = [(btn_x, btn_y+s*C['switch_post_spacing']/2) for s in (-1, 1)]
sw_pins = [(btn_x+a*C['switch_terminal_pitch_wide']/2, btn_y+b*C['switch_terminal_pitch_narrow']/2)
           for a in (-1, 1) for b in (-1, 1)]
sw_bosses = [(btn_x, btn_y+s*C['switch_boss_spacing']/2) for s in (-1, 1)]
led_x, led_y = C['led_x'], lens_y
led_collar_h = round(C['led_height'] - C['led_flange_thickness'] - LT + C['led_tip_recess'], 3)
led_seat = RIM - led_collar_h                   # the LED's flange stops here
ear_d, ear_off = C['lid_ear_diameter'], C['lid_ear_offset']
ear_ys = (C['lid_ear_bottom_y'], round((Y0 + 45.25 + pi_end) / 2, 2), L - C['lid_ear_inset_from_end'])
ears = [(x, y) for x in (-ear_off, W + ear_off) for y in ear_ys]

# FPC ribbon: up out of the Pi's CSI socket, back toward the microSD end over
# the board, a U-turn, then straight over the USB stack into the camera. The
# length of the fold-back is whatever uses up fpc_length.
R = C['fpc_bend_radius']
fpc_z1 = cam_back - C['camera_connector_slot_below_pcb']       # upper run
fpc_z0 = fpc_z1 - 2*R                                          # lower run
fpc_ys, fpc_zs = Y0 + CSI['px'], ZP + 1.5                      # start, inside the CSI socket
fpc_ye = cam_y0 + C['fpc_insertion'] - .5                      # end, inside the camera connector
fixed = (fpc_z0 - fpc_zs) + 2*R + (fpc_ye - fpc_ys) - 3*(2*R - math.pi*R/2)
fold = (C['fpc_length'] - fixed) / 2                           # how far it runs back
fpc_yb = fpc_ys - fold
fpc_pts = [(fpc_ys, fpc_zs), (fpc_ys, fpc_z0), (fpc_yb, fpc_z0), (fpc_yb, fpc_z1), (fpc_ye, fpc_z1)]

def fillet_path(pts, r, step=10):
    out = [pts[0]]
    for a, p, b in zip(pts, pts[1:], pts[2:]):
        d1 = (p[0]-a[0], p[1]-a[1]); n1 = math.hypot(*d1); d1 = (d1[0]/n1, d1[1]/n1)
        d2 = (b[0]-p[0], b[1]-p[1]); n2 = math.hypot(*d2); d2 = (d2[0]/n2, d2[1]/n2)
        assert abs(d1[0]*d2[0] + d1[1]*d2[1]) < 1e-9 and n1 >= r - 1e-6 and n2 >= r - 1e-6, 'right-angle corners only'
        c = (p[0]-d1[0]*r+d2[0]*r, p[1]-d1[1]*r+d2[1]*r)
        for k in range(0, 90 + step, step):
            t = math.radians(k)
            out.append((c[0] + r*(-d2[0]*math.cos(t) + d1[0]*math.sin(t)),
                        c[1] + r*(-d2[1]*math.cos(t) + d1[1]*math.sin(t))))
    out.append(pts[-1])
    return [p for i, p in enumerate(out) if i == 0 or math.dist(p, out[i-1]) > 1e-6]

fpc_line = fillet_path(fpc_pts, R)
fpc_len = sum(math.dist(a, b) for a, b in zip(fpc_line, fpc_line[1:]))

assert fold >= R, ('ribbon too short for this layout', C['fpc_length'], round(fixed + 2*R, 1))
assert fpc_yb > T + 1.0, ('ribbon too long: the fold-back hits the microSD-end wall', fpc_yb)
assert fpc_z0 - R > ZP + CSI['h'], 'first ribbon bend starts inside the CSI socket'
assert fpc_z1 > ZP + USB_A[0]['h'] + .8, ('ribbon rubs the USB stack', fpc_z1, ZP + USB_A[0]['h'])
assert R >= 3.0, 'bend radius below 3 mm'
assert cam_back - C['camera_connector_height'] > ZP + 2.0, 'camera connector too low'
assert RIM - sw_tip <= 15.0, ('switch reaches more than 15 mm behind the lid', RIM - sw_tip)
assert cap_bottom - (sw_seat + C['switch_body_height']) > C['switch_travel'] + 1.0, 'cap flange bottoms out on the switch body'
assert C['cap_proud'] > C['switch_travel'] + .5, 'cap goes below the lid face when pressed'
assert (C['cap_flange_diameter'] - C['cap_hole'])/2 >= 1.2, 'cap flange too small to stay captive'
assert C['switch_post_spacing']/2 - C['camera_post_diameter']/2 > max(C['switch_body'], C['cap_flange_diameter'])/2 + .4, 'switch posts too close'
assert led_x - C['led_collar_diameter']/2 > btn_x + C['carrier_width']/2, 'LED collar hits the switch carrier'
assert cam_x - CBW/2 - (led_x + C['led_collar_diameter']/2) > 1.0, 'LED hits the camera'
assert led_collar_h >= 0, 'LED shorter than the lid is thick'
assert C['led_window'] < C['led_diameter'] - .4, 'LED window too wide to stop the dome'
assert RIM - (ZP + C['gpio_jumper_height']) > C['lid_lip_depth'], 'GPIO jumpers reach the lid'

# TRAY: flat back on the bed, four Pi posts, three screw ears per side.
r = C['corner_radius']
tray = roundbox(0, 0, 0, W, L, RIM, r).cut(roundbox(T, T, T, W-2*T, L-2*T, RIM, max(r-T, .5)))
tray = union([tray] +
    [cylinder(x, y, T-.01, C['pi_post_diameter'], C['pi_post_height']+.01) for x, y in pi_holes] +
    [cylinder(x, y, 0, ear_d, RIM) for x, y in ears])
cuts = [cylinder(x, y, RIM-C['lid_screw_depth'], C['case_pilot'], C['lid_screw_depth']+1) for x, y in ears]
cuts += [cylinder(x, y, 1.0, C['pi_pilot'], C['pi_post_height']+T) for x, y in pi_holes]
# Port windows in the right wall.
usb_y, usb_z = Y0 + USBC['px'], ZP + USBC['h']/2
uw, uh = C['usbc_window_width'], C['usbc_window_height']
cuts += [box(W-T-1, usb_y-uw/2, usb_z-uh/2, T+2, uw, uh)]
hw, hh = C['hdmi_window_width'], C['hdmi_window_height']
if C['hdmi_windows']:
    cuts += [box(W-T-1, Y0+p['px']-hw/2, ZP+p['h']/2-hh/2, T+2, hw, hh) for p in HDMI]
# microSD: slot through the bottom wall plus a finger scoop in the back edge.
sd_x = X0 - SD_PY
sd_top = ZP - PI_T + .4
cuts += [box(sd_x-C['sd_slot_width']/2, -1, T+1.0, C['sd_slot_width'], T+2, sd_top-T-1.0),
         cylinder(sd_x, -3.0, -1, 2*C['sd_finger_radius'], sd_top+1)]
# Vents: vertical slits, GPIO-side wall and both end walls (a chimney when hung).
vw, (vz0, vz1) = C['vent_width'], C['vent_z']
cuts += [box(-1, Y0+px-vw/2, vz0, T+2, vw, vz1-vz0) for px in C['vent_side_px']]
cuts += [box(x-vw/2, L-T-1, vz0, vw, T+2, vz1-vz0) for x in C['vent_top_x']]
cuts += [box(x-vw/2, -1, vz0, vw, T+2, vz1-vz0) for x in C['vent_bottom_x']]
tray = add('Tray', subtract(tray, cuts), (.28, .34, .38),
    note='Print back face on the bed, no supports (windows bridge 13.5 mm). Pi: four M2.5 x 6 '
         'thread-forming screws. Lid: six M3 x 10 thread-forming screws into the ears.')

# LID: full cover, locating lip, camera posts and lens collar on the inside.
lc, lw, ld = C['lid_lip_clearance'], C['lid_lip_width'], C['lid_lip_depth']
lip = roundbox(T+lc, T+lc, RIM-ld, W-2*T-2*lc, L-2*T-2*lc, ld+.01, max(r-T-lc, .5)).cut(
      roundbox(T+lc+lw, T+lc+lw, RIM-ld-1, W-2*T-2*lc-2*lw, L-2*T-2*lc-2*lw, ld+2, max(r-T-lc-lw, .5)))
ci = C['camera_lens_block'] + 2*C['camera_collar_clearance']
co, chh = ci + 2*C['camera_collar_wall'], C['camera_collar_height']
collar = box(cam_x-co/2, lens_y-co/2, RIM-chh, co, co, chh+.01).cut(box(cam_x-ci/2, lens_y-ci/2, RIM-chh-1, ci, ci, chh+1))
lid = union([roundbox(0, 0, RIM, W, L, LT, r), lip, collar] +
            [cylinder(x, y, RIM, ear_d, LT) for x, y in ears] +
            [cylinder(x, y, cam_front, C['camera_post_diameter'], cam_post_h+.01) for x, y in cam_holes] +
            [cylinder(x, y, sw_seat, C['camera_post_diameter'], sw_post_h+.01) for x, y in sw_posts] +
            ([cylinder(led_x, led_y, led_seat, C['led_collar_diameter'], led_collar_h+.01)] if led_collar_h > 0 else []))
bore, cham = C['camera_lens_bore'], C['camera_lens_chamfer']
cuts = [cylinder(x, y, RIM-1, C['lid_screw_clearance'], LT+2) for x, y in ears]
cuts += [cylinder(x, y, cam_front-.1, C['camera_pilot'], cam_post_h+LT-.8+.1) for x, y in cam_holes]
cuts += [cylinder(cam_x, lens_y, RIM-.1, bore, LT+.2),
         Part.makeCone(bore/2, bore/2+cham+.01, cham+.01, V(cam_x, lens_y, H-cham)),
         cylinder(btn_x, btn_y, RIM-1, C['cap_hole'], LT+2),
         cylinder(led_x, led_y, led_seat-1, C['led_hole'], led_collar_h+LT-C['led_window_lip']+1),
         cylinder(led_x, led_y, RIM, C['led_window'], LT+1)]
cuts += [cylinder(x, y, sw_seat-.1, C['camera_pilot'], sw_post_h+LT-.8+.1) for x, y in sw_posts]
lid = add('Lid', subtract(lid, cuts), (.91, .90, .85), flip=True,
    note='Print outer face on the bed, posts up, no supports. Camera: four M2 x 6 thread-forming screws. '
         'Switch carrier: two M2 x 6. The LED pushes into its collar from inside.')

# BUTTON CAP: stem through the lid, flange under it, shallow socket over the plunger.
pl = C['switch_plunger_head'] + 2*C['cap_socket_clearance']
cap = cylinder(btn_x, btn_y, cap_bottom, C['cap_flange_diameter'], C['cap_flange_thickness']).fuse(
      cylinder(btn_x, btn_y, cap_top-.01, C['cap_diameter'], H+C['cap_proud']-cap_top+.01))
cap = add('ButtonCap', subtract(cap, [box(btn_x-pl/2, btn_y-pl/2, cap_bottom-1, pl, pl, C['cap_socket_depth']+1)]), (.91, .90, .85),
    note='Print flange on the bed, no supports. Drop it into the lid hole from inside before the switch goes on.')
# SWITCH CARRIER: plate under the switch, screwed to two lid posts; terminals pass through slots.
cw, cl, ct = C['carrier_width'], C['switch_post_spacing'] + C['carrier_end_margin']*2, C['carrier_thickness']
sx, sy = C['carrier_slot']
carrier = roundbox(btn_x-cw/2, btn_y-cl/2, sw_seat-ct, cw, cl, ct, 2.0)
carrier = add('ButtonCarrier', subtract(carrier, [cylinder(x, y, sw_seat-ct-1, C['carrier_screw_clearance'], ct+2) for x, y in sw_posts] +
    [box(x-sx/2, y-sy/2, sw_seat-ct-1, sx, sy, ct+2) for x, y in sw_pins] +
    [cylinder(x, y, sw_seat-ct-1, C['carrier_boss_hole'], ct+2) for x, y in sw_bosses]), (.28, .34, .38),
    note='Print flat, no supports. Switch terminals go through the four slots; two M2 x 6 screws into the lid posts.')

# REFERENCES -----------------------------------------------------------------
GREEN, SILVER, BLACK, DARK = (.10, .42, .24), (.72, .73, .74), (.10, .10, .10), (.22, .22, .24)
pcb = roundbox(X0-PI_W, Y0, ZP-PI_T, PI_W, PI_L, PI_T, PI_R)
add('PiPCB', subtract(pcb, [cylinder(x, y, ZP-PI_T-1, 2.7, PI_T+2) for x, y in pi_holes]), GREEN, REF,
    note='Raspberry Pi 4 Model B PCB, 85 x 56, components toward the lid.')
edge = lambda p: pbox(p['px']-p['w']/2, p['px']+p['w']/2, -p['over'], p['depth']-p['over'], 0, p['h'])
end = lambda p: pbox(PI_L+p['over']-p['depth'], PI_L+p['over'], p['py']-p['w']/2, p['py']+p['w']/2, 0, p['h'])
audio = pbox(AUDIO['px']-3.5, AUDIO['px']+3.5, 0, AUDIO['depth'], 0, AUDIO['h']).fuse(
    cylinder(X0, Y0+AUDIO['px'], ZP+3.0, 6.0, AUDIO['over'], (1, 0, 0)))
add('PiPorts', Part.makeCompound([edge(USBC), edge(HDMI[0]), edge(HDMI[1]), audio, end(USB_A[0]), end(USB_A[1]), end(ETH)]),
    SILVER, REF, note='USB-C, 2 x micro-HDMI, audio, 2 x USB-A stacks, Ethernet.')
add('PiHeaders', Part.makeCompound([
    pbox(GPIO['px']-GPIO['l']/2, GPIO['px']+GPIO['l']/2, GPIO['py']-GPIO['w']/2, GPIO['py']+GPIO['w']/2, 0, GPIO['h']),
    pbox(CSI['px']-CSI['w']/2, CSI['px']+CSI['w']/2, CSI['py']-CSI['l']/2, CSI['py']+CSI['l']/2, 0, CSI['h']),
    pbox(DSI['px']-DSI['w']/2, DSI['px']+DSI['w']/2, DSI['py']-DSI['l']/2, DSI['py']+DSI['l']/2, 0, DSI['h']),
    pbox(59.0, 64.0, 43.9, 48.9, 0, 8.5)]), BLACK, REF, note='GPIO 2x20, CSI, DSI, PoE header.')
add('PiChips', Part.makeCompound([pbox(21.75, 36.75, 25.0, 40.0, 0, 2.4), pbox(39.5, 51.5, 26.5, 38.5, 0, 1.2),
    pbox(6.5, 18.5, 33.0, 45.0, 0, 1.6), pbox(55.0, 61.0, 29.5, 35.5, 0, 1.0)]), DARK, REF,
    note='SoC (dimensioned) and RAM, radio can, USB controller (approximate).')
add('PiSDCard', Part.makeCompound([pbox(0.5, 14.0, SD_PY-6.5, SD_PY+6.5, -PI_T-1.6, -PI_T),
    pbox(-SD_OVER, 0.5, SD_PY-SD_CARD_W/2, SD_PY+SD_CARD_W/2, -PI_T-1.3, -PI_T-.3)]), (.75, .2, .15), REF,
    note='microSD socket under the board and the card sticking out 2.5 mm.')
under = subtract(roundbox(X0-PI_W, Y0, ZP-PI_T-C['pi_underside_allowance'], PI_W, PI_L, C['pi_underside_allowance'], PI_R),
                 [cylinder(x, y, 0, C['pi_post_diameter']+.5, ZP) for x, y in pi_holes])
add('PiUndersideAllowance', under, (.55, .45, .25), REF, note='Pin tails and parts under the board.')
add('GPIOJumperAllowance', pbox(GPIO['px']-GPIO['l']/2, GPIO['px']+GPIO['l']/2, GPIO['py']-GPIO['w']/2,
    GPIO['py']+GPIO['w']/2, GPIO['h'], C['gpio_jumper_height']), (.85, .75, .35), REF,
    note='Dupont housings on the GPIO pins plus the wire bend.')
add('PowerPlugAllowance', box(X0+USBC['over']+.2, usb_y-6.5, usb_z-3.5, W+30-X0-USBC['over']-.2, 13, 7), (.23, .29, .32), REF,
    note='Straight USB-C plug, overmold up to 13 x 7 mm, 30 mm of plug and cable past the wall.')
add('HDMIPlugAllowance', Part.makeCompound([box(X0+p['over']+.2, Y0+p['px']-5.5, ZP+p['h']/2-3.25,
    W+20-X0-p['over']-.2, 11, 6.5) for p in HDMI]), (.23, .29, .32), REF, note='Slim micro-HDMI plugs, overmold up to 11 x 6.5 mm.')

cam = subtract(roundbox(cam_x-CBW/2, cam_y0, cam_back, CBW, CBL, CBT, 2.0),
               [cylinder(x, y, cam_back-1, 2.2, CBT+2) for x, y in cam_holes])
add('CameraPCB', cam, GREEN, REF, note='Arducam IMX519 (B0371), 25 x 23.86 mm, lens toward the lid.')
lb = C['camera_lens_block']
lens_top = cam_front + C['camera_lens_block_height']
add('CameraLens', box(cam_x-lb/2, lens_y-lb/2, cam_front, lb, lb, C['camera_lens_block_height']).fuse(
    cylinder(cam_x, lens_y, lens_top-.01, C['camera_lens_barrel_diameter'], C['camera_lens_barrel_height']+.01)),
    BLACK, REF, note='Autofocus block 9.5 x 9.5 x 5 and the 7.5 mm barrel standing 1 mm proud.')
add('CameraFrontParts', Part.makeCompound([box(cam_x-4.75, cam_y0, cam_front, 9.5, 5.0-.05, 1.8),
    box(cam_x-5.0, lens_y+lb/2+.05, cam_front, 10.0, 2.49, 2.9)]), DARK, REF,
    note='Sensor-flex connector and flex fold beside the lens block.')
add('CameraConnector', box(cam_x-11, cam_y0-C['camera_connector_overhang'], cam_back-C['camera_connector_height'], 22,
    C['camera_connector_depth'], C['camera_connector_height']), (.85, .85, .80), REF,
    note='15-pin FPC connector on the camera back, on the edge facing the Pi.')

ft = 0.3
def offset(line, d):
    pts = []
    for i, p in enumerate(line):
        a, b = line[max(i-1, 0)], line[min(i+1, len(line)-1)]
        n = math.hypot(b[0]-a[0], b[1]-a[1])
        pts.append((p[0] - d*(b[1]-a[1])/n, p[1] + d*(b[0]-a[0])/n))
    return pts
loop = offset(fpc_line, ft/2) + offset(fpc_line, -ft/2)[::-1]
wire = Part.makePolygon([V(cam_x-C['fpc_width']/2, y, z) for y, z in loop + loop[:1]])
add('CSIRibbon', Part.Face(wire).extrude(V(C['fpc_width'], 0, 0)), (.92, .90, .82), REF,
    note='15-pin 1.0 mm FPC, drawn 0.3 mm thick along its real path.')

sb = C['switch_body']
tw, tt = C['switch_terminal_section']
ph, pt = C['switch_plunger_head'], C['switch_plunger_head_height']
switch = union([box(btn_x-sb/2, btn_y-sb/2, sw_seat, sb, sb, C['switch_body_height']),
                cylinder(btn_x, btn_y, sw_seat+C['switch_body_height']-.01, C['switch_plunger_base_diameter'],
                         C['switch_plunger_base_height']-C['switch_body_height']+.01),
                cylinder(btn_x, btn_y, sw_seat+C['switch_plunger_base_height']-.01, 3.0, C['switch_height']-pt-C['switch_plunger_base_height']+.02),
                box(btn_x-ph/2, btn_y-ph/2, sw_top-pt, ph, ph, pt)] +
               [box(x-tt/2, y-tw/2, sw_tip, tt, tw, C['switch_terminal_length']+.5) for x, y in sw_pins] +
               [cylinder(x, y, sw_seat-1.0, C['switch_boss_diameter'], 1.01) for x, y in sw_bosses])
add('Switch', switch, DARK, REF,
    note='Omron B3F-4055: 12 x 12 mm tactile switch, 3.8 mm square projected plunger, 7.3 mm tall, four terminals 3.5 mm long. '
         'Boss length (1 mm) and neck width (3 mm) are not on the drawing.')
lr = C['led_diameter']/2
led_tip = led_seat + C['led_height'] - C['led_flange_thickness']
add('StatusLED', union([cylinder(led_x, led_y, led_seat, C['led_diameter'], led_tip-lr-led_seat+.01),
                        Part.makeSphere(lr, V(led_x, led_y, led_tip-lr)),
                        cylinder(led_x, led_y, led_seat-C['led_flange_thickness'], C['led_flange_diameter'], C['led_flange_thickness']+.01)] +
                       [box(led_x+a*C['led_lead_spacing']/2-.25, led_y-.25, led_seat-C['led_flange_thickness']-C['led_lead_allowance'],
                            .5, .5, C['led_lead_allowance']+.01) for a in (-1, 1)]), (.25, .75, .35), REF,
    note='Kingbright WP710A10SGD 3 mm LED pushed into the lid from inside until the dome meets the 2.2 mm window; '
         'leads bent over within led_lead_allowance. The dome is drawn as a hemisphere; its radius is not on the drawing.')

# COUPONS: slices of the real parts.
c1 = tray.common(box(-10, -5, 0, W+20, Y0+69+5, ZP+7.0)).cut(box(13, 13, -1, W-26, Y0+47, T+2))
add('PiFitCoupon', c1, (.28, .34, .38), FIT,
    note='Bottom 15 mm of the Tray up to the upper Pi posts: hole pattern, USB-C and HDMI windows, microSD slot.')
c2 = lid.common(box(-10, pi_end+1.0, RIM-10, W+20, L, LT+10))
add('LidFitCoupon', c2, (.91, .90, .85), FIT, flip=True,
    note='Camera end of the Lid: camera posts, lens bore and collar, cap hole, switch posts, LED collar.')

DOC.recompute()

def print_shape(record):
    s = record['obj'].Shape.copy()
    if record['flip']:
        s.rotate(V(0, 0, 0), V(0, 1, 0), 180)
    b = s.BoundBox
    s.translate(V(-b.XMin, -b.YMin, -b.ZMin))
    return s

def mesh_record(name, kind, color, note, shape, deflection=.15):
    mesh = MeshPart.meshFromShape(Shape=shape, LinearDeflection=deflection, AngularDeflection=.35, Relative=False)
    vertices, faces = mesh.Topology
    return dict(name=name, kind=kind, color=color, note=note,
                vertices=[[round(p.x, 3), round(p.y, 3), round(p.z, 3)] for p in vertices],
                faces=[list(f) for f in faces])

scene, manifest, plate = [], [], []
section = box(cam_x, -50, -50, 200, L+100, H+100)        # everything right of the ribbon centreline
for rec in parts + refs + coupons:
    s = rec['obj'].Shape
    kind = 'part' if rec in parts else 'reference' if rec in refs else 'coupon'
    scene.append(mesh_record(rec['name'], kind, rec['color'], rec['note'], s))
    if kind != 'coupon':
        scene.append(mesh_record(rec['name']+'#cut', 'section', rec['color'], rec['note'], s.cut(section)))
    if kind == 'reference':
        continue
    ps = print_shape(rec)
    pm = MeshPart.meshFromShape(Shape=ps, LinearDeflection=.08, AngularDeflection=.16, Relative=False)
    assert pm.isSolid(), rec['name']
    pm.write(str(OUT/(rec['name']+'.stl')))
    s.exportStep(str(OUT/(rec['name']+'.step')))
    b = ps.BoundBox
    if kind == 'part':
        plate.append((rec['name'], pm))
    manifest.append(dict(name=rec['name'], kind=kind, solid_count=len(s.Solids), valid=s.isValid(),
                         closed_mesh=pm.isSolid(), volume_mm3=round(s.Volume, 3),
                         print_size_mm=[round(v, 3) for v in (b.XLength, b.YLength, b.ZLength)], note=rec['note']))

# Plain model 3MF: Tray and Lid side by side in print orientation on a 256 mm bed.
def write_3mf(path, items, gap=12.0, bed=256.0):
    width = sum(m.BoundBox.XLength for _, m in items) + gap*(len(items)-1)
    x = (bed - width) / 2
    objects, build = [], []
    for i, (name, m) in enumerate(items, 1):
        vertices, faces = m.Topology
        vs = ''.join(f'<vertex x="{p.x:.4f}" y="{p.y:.4f}" z="{p.z:.4f}"/>' for p in vertices)
        ts = ''.join(f'<triangle v1="{a}" v2="{b}" v3="{c}"/>' for a, b, c in faces)
        objects.append(f'<object id="{i}" name="{name}" type="model"><mesh><vertices>{vs}</vertices>'
                       f'<triangles>{ts}</triangles></mesh></object>')
        y = (bed - m.BoundBox.YLength) / 2
        build.append(f'<item objectid="{i}" transform="1 0 0 0 1 0 0 0 1 {x:.3f} {y:.3f} 0"/>')
        x += m.BoundBox.XLength + gap
    model = ('<?xml version="1.0" encoding="UTF-8"?>\n<model unit="millimeter" xml:lang="en-US" '
             'xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">'
             '<metadata name="Title">Daily Mirror Pi 4 enclosure</metadata>'
             f'<resources>{"".join(objects)}</resources><build>{"".join(build)}</build></model>')
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr('[Content_Types].xml', '<?xml version="1.0" encoding="UTF-8"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
                   '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
                   '<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>')
        z.writestr('_rels/.rels', '<?xml version="1.0" encoding="UTF-8"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                   '<Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>')
        z.writestr('3D/3dmodel.model', model)
write_3mf(OUT/'TrayAndLid.3mf', plate)

checks = []
def clearance(name, a, b, allowance=1e-5):
    v = a.common(b).Volume
    checks.append(dict(name=name, overlap_mm3=round(v, 8), passed=v < allowance))
    assert v < allowance, (name, v)

named = {r['name']: r['obj'].Shape for r in refs}
clearance('Tray / Lid', tray, lid)
for n in named:
    clearance(n+' / Tray', named[n], tray)
    clearance(n+' / Lid', named[n], lid)
pi_names = ('PiPCB', 'PiPorts', 'PiHeaders', 'PiChips', 'GPIOJumperAllowance')
cam_names = ('CameraPCB', 'CameraLens', 'CameraFrontParts', 'CameraConnector')
for a in ('CSIRibbon', 'Switch', 'StatusLED', 'ButtonCarrier', 'ButtonCap') + cam_names:
    for b in pi_names:
        named.setdefault('ButtonCarrier', carrier); named.setdefault('ButtonCap', cap)
        if (a, b) != ('CSIRibbon', 'PiHeaders'):           # the ribbon starts inside the CSI socket
            clearance(a+' / '+b, named[a], named[b])
for b in ('CameraPCB', 'CameraLens', 'CameraFrontParts'):
    clearance('CSIRibbon / '+b, named['CSIRibbon'], named[b])
for a in ('Switch', 'StatusLED', 'ButtonCarrier', 'ButtonCap'):
    for b in cam_names + ('CSIRibbon',):
        clearance(a+' / '+b, named[a], named[b])
for a, b in (('ButtonCap', 'Switch'), ('ButtonCarrier', 'Switch'), ('ButtonCap', 'ButtonCarrier'), ('StatusLED', 'Switch'),
             ('StatusLED', 'ButtonCarrier'), ('StatusLED', 'ButtonCap')):
    clearance(a+' / '+b, named[a], named[b])
for a in ('ButtonCap', 'ButtonCarrier'):
    clearance(a+' / Tray', named[a], tray)
    clearance(a+' / Lid', named[a], lid)
# The ribbon may only touch the CSI socket itself among the headers.
others = Part.makeCompound(named['PiHeaders'].Solids[:1] + named['PiHeaders'].Solids[2:])
clearance('CSIRibbon / GPIO, DSI, PoE', named['CSIRibbon'], others)

layout = dict(outer_mm=[W, L, H], outer_with_ears_mm=[round(W+2*(ear_off+ear_d/2), 2), L, H],
              rim_z=RIM, inner_mm=[W-2*T, round(L-2*T, 2), RIM-T], pi_origin=[X0, Y0, ZP],
              pi_holes=[[round(x, 3), round(y, 3)] for x, y in pi_holes], pi_end_y=pi_end,
              headroom_over_pcb=round(RIM-ZP, 3), headroom_over_usb=round(RIM-ZP-USB_A[0]['h'], 3),
              usb_c_centre=[W, usb_y, usb_z], hdmi_centres=[[W, Y0+p['px'], ZP+p['h']/2] for p in HDMI],
              sd_slot_centre=[sd_x, 0, round((T+1.0+sd_top)/2, 3)],
              camera_centre_x=cam_x, camera_board_y=[cam_y0, round(cam_y1, 3)], camera_pcb_z=[round(cam_back, 3), round(cam_front, 3)],
              camera_holes=[[round(x, 3), round(y, 3)] for x, y in cam_holes], camera_post_height=cam_post_h,
              lens_centre=[cam_x, round(lens_y, 3)], lens_barrel_top_z=round(lens_top+C['camera_lens_barrel_height'], 3),
              lens_recess_below_face=round(H-lens_top-C['camera_lens_barrel_height'], 3),
              button_centre=[btn_x, round(btn_y, 3)], switch_seat_z=round(sw_seat, 3), switch_post_height=round(sw_post_h, 3),
              switch_posts=[[round(x, 3), round(y, 3)] for x, y in sw_posts],
              switch_depth_behind_lid=round(RIM-sw_tip, 3), switch_depth_behind_lid_face=round(H-sw_tip, 3),
              cap_proud=C['cap_proud'], cap_rest_gap=C['cap_rest_gap'],
              led_centre=[led_x, round(led_y, 3)], led_collar_height=led_collar_h,
              led_depth_behind_lid=round(led_collar_h+C['led_flange_thickness']+C['led_lead_allowance'], 3),
              fpc=dict(length=round(fpc_len, 2), bend_radius=R, lower_run_z=round(fpc_z0, 3), upper_run_z=round(fpc_z1, 3),
                       fold_back_to_y=round(fpc_yb, 3), fold_back_to_px=round(fpc_yb-Y0, 3),
                       min_length=round(fixed+2*R, 1), max_length=round(fixed+2*(fpc_ys-T-1.0), 1),
                       path_yz=[[round(y, 3), round(z, 3)] for y, z in fpc_pts]),
              ears=ears, section_x=cam_x)
(OUT/'layout.json').write_text(json.dumps(layout, indent=2)+'\n')
DOC.saveAs(str(OUT/'DailyMirror-Pi4.FCStd'))
Part.makeCompound([r['obj'].Shape for r in parts]).exportStep(str(OUT/'Assembly.step'))
(OUT/'scene.json').write_text(json.dumps(scene))
(OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
report = dict(status='CAD validated; physical fit untested', freecad=App.Version()[:3], units='mm',
              layout=layout, checks=checks, parts=manifest)
(OUT/'validation.json').write_text(json.dumps(report, indent=2)+'\n')
print('BUILD_OK', len(parts), 'printable parts;', len(coupons), 'coupons;', len(checks), 'clearance checks')
print('LAYOUT', json.dumps({k: layout[k] for k in ('outer_mm', 'outer_with_ears_mm', 'inner_mm', 'headroom_over_pcb',
      'headroom_over_usb', 'lens_centre', 'button_centre', 'switch_depth_behind_lid', 'led_centre', 'led_collar_height', 'fpc')}))
App.closeDocument(DOC.Name)
