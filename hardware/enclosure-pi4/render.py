"""Render previews from the actual CAD meshes in output/scene.json (no GUI needed).

Writes exterior.png, cutaway.png, exploded.png, lid-off.png and the combined
enclosure-preview.png. The cutaway uses the '#cut' meshes build.py makes by
slicing every part on the ribbon/lens centreline.
"""
from pathlib import Path
import json
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'output'
scene = {s['name']: s for s in json.loads((OUT / 'scene.json').read_text())}
lay = json.loads((OUT / 'layout.json').read_text())
bg, ink = '#eeede7', '#243b44'
SS = 2      # supersampling

def raster(items, elev, azim, width=900, height=900, focus=None, project=False, edges=False):
    """items: (name, (dx, dy, dz)) pairs. Per-pixel depth buffer, flat shading.
    focus: optional model-space box (x0, x1, y0, y1, z0, z1) to frame instead of
    the whole scene. project=True also returns a function mapping model points
    to pixel coordinates of the returned image (for labels)."""
    yaw, pitch = np.radians([-(azim + 90), elev])
    cy, sy, cp, sp = np.cos(yaw), np.sin(yaw), np.cos(pitch), np.sin(pitch)
    # Model Z (out of the wall) is drawn up; the camera orbits around it.
    rotation = np.array([[cy, 0, sy], [sp*sy, cp, -sp*cy], [-cp*sy, sp, cp*cy]])
    swap = np.array([[1, 0, 0], [0, 0, 1], [0, -1, 0]])       # model (x, y, z) -> (x, z, -y)
    light = np.array([-.35, .65, 1]); light /= np.linalg.norm(light)
    triangles, colors = [], []
    for name, shift in items:
        s = scene[name]
        if not s['faces']:
            continue
        v = (np.array(s['vertices']) + np.array(shift)) @ swap.T
        tris = v[np.array(s['faces'])] @ rotation.T
        normal = np.cross(tris[:, 1]-tris[:, 0], tris[:, 2]-tris[:, 0])
        normal /= np.maximum(np.linalg.norm(normal, axis=1)[:, None], 1e-8)
        intensity = .50 + .50*np.abs(normal @ light)
        colors.extend(np.clip(np.array(s['color'])[None, :]*intensity[:, None], 0, 1))
        triangles.extend(tris)
    tris = np.array(triangles); colors = np.array(colors)
    lo = tris.min(axis=(0, 1)); hi = tris.max(axis=(0, 1))
    if focus is not None:
        corners = np.array([[x, y, z] for x in focus[:2] for y in focus[2:4] for z in focus[4:]]) @ swap.T @ rotation.T
        lo[:2], hi[:2] = corners.min(axis=0)[:2], corners.max(axis=0)[:2]
    w, h = width*SS, height*SS
    scale = min((w-80*SS)/(hi[0]-lo[0]), (h-60*SS)/(hi[1]-lo[1]))
    tris[:, :, 0] = (tris[:, :, 0]-(hi[0]+lo[0])/2)*scale + w/2
    tris[:, :, 1] = h/2 - (tris[:, :, 1]-(hi[1]+lo[1])/2)*scale
    pixels = np.ones((h, w, 3), dtype=np.float32)*np.array(matplotlib.colors.to_rgb(bg), dtype=np.float32)
    depth = np.full((h, w), -np.inf)
    for tri, color in zip(tris, colors):
        x0, y0 = np.maximum(np.floor(tri[:, :2].min(0)).astype(int), 0)
        x1, y1 = np.minimum(np.ceil(tri[:, :2].max(0)).astype(int), [w-1, h-1])
        if x1 < x0 or y1 < y0:
            continue
        a, b, c = tri
        den = (b[1]-c[1])*(a[0]-c[0]) + (c[0]-b[0])*(a[1]-c[1])
        if abs(den) < 1e-8:
            continue
        xx = np.arange(x0, x1+1)[None, :] + .5; yy = np.arange(y0, y1+1)[:, None] + .5
        wa = ((b[1]-c[1])*(xx-c[0]) + (c[0]-b[0])*(yy-c[1]))/den
        wb = ((c[1]-a[1])*(xx-c[0]) + (a[0]-c[0])*(yy-c[1]))/den
        wc = 1 - wa - wb
        zz = wa*a[2] + wb*b[2] + wc*c[2]
        target = depth[y0:y1+1, x0:x1+1]
        mask = (wa >= -1e-6) & (wb >= -1e-6) & (wc >= -1e-6) & (zz > target)
        target[mask] = zz[mask]
        pixels[y0:y1+1, x0:x1+1][mask] = color
    if edges:       # outline depth steps (post tops on a flat plate, hole rims) in ink
        d = np.where(np.isfinite(depth), depth, -1e6)
        step = np.zeros_like(d, dtype=bool)
        step[:, :-1] |= np.abs(d[:, 1:] - d[:, :-1]) > .35
        step[:-1, :] |= np.abs(d[1:, :] - d[:-1, :]) > .35
        step[1:, :] |= step[:-1, :].copy(); step[:, 1:] |= step[:, :-1].copy()
        pixels[step] = pixels[step]*.25 + np.array(matplotlib.colors.to_rgb(ink), dtype=np.float32)*.75
    image = pixels.reshape(height, SS, width, SS, 3).mean(axis=(1, 3))
    if not project:
        return image
    mid = (hi + lo) / 2
    def to_pixel(p):
        v = np.array(p, dtype=float) @ swap.T @ rotation.T
        return ((v[0]-mid[0])*scale + w/2) / SS, (h/2 - (v[1]-mid[1])*scale) / SS
    return image, to_pixel

def at(names, shift=(0, 0, 0), cut=False):
    return [(n + '#cut' if cut else n, shift) for n in names]

PI = ['PiPCB', 'PiPorts', 'PiHeaders', 'PiChips', 'PiSDCard']
CAM = ['CameraPCB', 'CameraLens', 'CameraFrontParts', 'CameraConnector']
w, l, h = lay['outer_mm']
head = 'DAILY MIRROR  /  Raspberry Pi 4 tray + lid'
sub = (f'{w:g} x {l:g} x {h:g} mm  /  Pi 4 Model B, Arducam IMX519 behind the lid, 12 mm switch + 5 mm RGB LED on a control PCB  /  '
       f'{lay["fpc"]["length"]:.0f} mm ribbon, {lay["fpc"]["bend_radius"]:g} mm bends')
foot = 'Rendered from the exported CAD geometry. The Pi, camera, ribbon, switch and LED are dimensioned stand-ins.'

def sheet(filename, panels, size=(16, 9)):
    fig = plt.figure(figsize=size, facecolor=bg)
    for i, (title, items, elev, azim) in enumerate(panels):
        ax = fig.add_subplot(1, len(panels), i+1)
        ax.imshow(raster(items, elev, azim))
        ax.set_axis_off()
        ax.set_title(title, loc='left', fontsize=11, fontweight='bold', color=ink, pad=0)
    fig.text(.045, .9, head, size=20, weight='bold', color=ink)
    fig.text(.045, .85, sub, size=12, color=ink)
    fig.text(.045, .07, foot, size=10, color=ink)
    plt.subplots_adjust(left=.035, right=.97, top=.80, bottom=.12, wspace=.02)
    fig.savefig(OUT / filename, dpi=120, facecolor=bg)
    plt.close(fig)

BTN = ['ButtonCap', 'ControlPCB', 'Switch', 'StatusLED', 'J1Header', 'ControlResistors']   # the PCB replaces the ButtonCarrier
closed = at(['Tray', 'Lid', 'ButtonCap', 'StatusLED', 'CameraLens'])
inside = at(['Tray', 'CSIRibbon'] + PI + CAM + BTN)
cutaway = at(['Tray', 'Lid', 'CSIRibbon'] + BTN + PI + CAM, cut=True)
exploded = (at(['Tray'] + PI) + at(['CSIRibbon'], (0, 0, 12)) +
            at(['Lid'] + CAM, (0, 0, 55)) + at(['Switch', 'StatusLED', 'ControlPCB', 'J1Header', 'ControlResistors'], (0, 0, 40)) +
            at(['ButtonCap'], (0, 0, 70)))
sheet('exterior.png', [('01   FRONT, PORT SIDE', closed, 35, -55), ('02   FRONT, VENT SIDE', closed, 35, -125),
                       ('03   BACK AND microSD END', closed, -35, -60)])
sheet('cutaway.png', [('SECTION ON THE RIBBON / LENS CENTRELINE', cutaway, 12, 0),
                      ('SAME SECTION, FROM ABOVE', cutaway, 40, -25)])
sheet('exploded.png', [('EXPLODED', exploded, 25, -55), ('LID OFF', inside, 50, -55),
                       ('LID UNDERSIDE', at(['Lid'] + CAM + BTN), -50, -120)])
sheet('lid-off.png', [('LID OFF, PORT SIDE', inside, 40, -40), ('LID OFF, GPIO SIDE', inside, 40, -140)])
sheet('enclosure-preview.png', [('01   ASSEMBLED', closed, 35, -55), ('02   SECTION', cutaway, 20, -15),
                                ('03   LID OFF', inside, 50, -55)])

# --- Control PCB and LED change: annotated views ------------------------------
C = json.loads((ROOT / 'dimensions.json').read_text())
pcb = lay['control_pcb']
bx, by = lay['button_centre']
lx, ly = lay['led_centre']
cx, cy = lay['lens_centre']
RIM, H = lay['rim_z'], h
seat, pbot = pcb['z'][1], pcb['z'][0]
NEW, OLD, CAMC = '#c2410c', '#1d4ed8', '#047857'
BAY = (-2, w + 2, 86, l + 1, 0, h)


def annotated(filename, title, subtitle, panels, size=(16, 9), foot_text=None):
    """panels: dicts with title, items, elev, azim, focus, labels [(text, point, (dx, dy), color)],
    lines [(p0, p1, text, color)] drawn in model space."""
    fig = plt.figure(figsize=size, facecolor=bg)
    n = len(panels)
    for i, pn in enumerate(panels):
        ax = fig.add_axes(pn.get('rect', (.03 + i*.95/n, .12, .95/n - .01, .70)))
        img, to_px = raster(pn['items'], pn['elev'], pn['azim'], pn.get('px', 1000), pn.get('px', 1000),
                            focus=pn.get('focus'), project=True, edges=True)
        ax.imshow(img)
        ax.set_anchor('N')
        ax.set_xlim(0, img.shape[1]); ax.set_ylim(img.shape[0], 0)
        ax.set_axis_off()
        ax.set_title(pn['title'], loc='left', fontsize=11, fontweight='bold', color=ink, pad=4)
        for p0, p1, text, color in pn.get('lines', []):
            (x0, y0), (x1, y1) = to_px(p0), to_px(p1)
            ax.plot([x0, x1], [y0, y1], color=color, lw=1.0, ls=(0, (4, 3)))
            if text:
                ax.text(x1 + 6, y1, text, color=color, fontsize=8.5, va='center', ha='left',
                        bbox=dict(boxstyle='round,pad=.15', fc=bg, ec='none', alpha=.85))
        for text, points, where, color in pn.get('labels', []):
            # points: one model point or a list; where: absolute (x, y) in panel pixels, or an offset
            # (dx, dy) from the first point when given as ('+', dx, dy).
            points = [points] if isinstance(points[0], (int, float)) else points
            pix = [to_px(q_) for q_ in points]
            tx, ty = (pix[0][0] + where[1], pix[0][1] + where[2]) if where[0] == '+' else where
            for k, (x, y) in enumerate(pix):
                ax.annotate(text if k == len(pix) - 1 else '', (x, y), xytext=(tx, ty), color=color, fontsize=9.5,
                            fontweight='bold', ha='center', va='center', zorder=5 + (k == len(pix) - 1),
                            arrowprops=dict(arrowstyle='-|>', color=color, lw=1.1, shrinkA=6, shrinkB=1),
                            bbox=dict(boxstyle='round,pad=.3', fc='white', ec=color, lw=.9, alpha=.95) if k == len(pix) - 1 else None)
        for p0, p1, text, color in pn.get('dims', []):
            (a, b), (c, d) = to_px(p0), to_px(p1)
            ax.annotate('', (a, b), xytext=(c, d), arrowprops=dict(arrowstyle='<|-|>', color=color, lw=1.1, shrinkA=0, shrinkB=0))
            ax.text(max(a, c) + 8, (b + d) / 2, text, color=color, fontsize=8.5, fontweight='bold', va='center',
                    bbox=dict(boxstyle='round,pad=.15', fc=bg, ec='none', alpha=.85))
    fig.text(.03, .93, title, size=19, weight='bold', color=ink)
    import textwrap
    fig.text(.03, .895, textwrap.fill(subtitle, int(size[0] * 10.5)), size=11.5, color=ink, va='top')
    fig.text(.03, .04, foot_text or ('Rendered from the exported CAD geometry (build.py). Orange = new in this change, blue = existing, '
             'green = camera. PCB, LED, switch, Pi and camera are dimensioned stand-ins.'), size=9.5, color=ink)
    fig.savefig(OUT / filename, dpi=120, facecolor=bg)
    plt.close(fig)


post_top = lambda x, y: (x, y, seat)
sw_p, led_p, cam_p = lay['switch_posts'], lay['led_posts'], lay['camera_holes']
lid_labels = [
    ('camera posts (existing)', [post_top(*c_) for c_ in cam_p], (270, 120), CAMC),
    ('button posts (existing)', [post_top(*q_) for q_ in sw_p], (760, 120), OLD),
    (f'LED posts (NEW): same {lay["switch_post_height"]:g} mm length, M2 pilot', [post_top(*q_) for q_ in led_p], (560, 900), NEW),
    ('5.5 mm LED hole + 7.5 mm\nlight collar (NEW)', (lx - 3.75, ly - 1.0, lay['led_collar_z'][0]), (400, 790), NEW),
    ('cap hole 10.6\n(existing)', (bx - 5.3, by - 1.5, RIM), (880, 780), OLD),
    ('lens bore +\ncollar', (cx + 4.25, cy - 2.0, RIM), (110, 790), CAMC),
]
inside = dict(elev=-90, azim=90, focus=BAY)
# (a) lid inside face: posts.
annotated('pcb-lid-posts.png', 'LID INSIDE FACE  -  posts for the control PCB',
          f'Seen from inside (mirror image of the front). Existing button posts at {sw_p[0]} and {sw_p[1]}; new LED posts at '
          f'{led_p[0]} and {led_p[1]}: same {lay["switch_post_height"]} mm length, M2 pilot, nothing moved.',
          [dict(title='CAMERA END OF THE LID, STRAIGHT ON', items=at(['Lid']), labels=lid_labels, **inside),
           dict(title='SAME, OBLIQUE', items=at(['Lid']), elev=-48, azim=120, focus=(-2, w + 2, 88, l + 1, 20, h),
                labels=[('LED posts (NEW)', [post_top(*q_) for q_ in led_p], (520, 880), NEW),
                        ('light collar (NEW)', (lx, ly - 3.75, lay['led_collar_z'][0]), (820, 860), NEW),
                        ('button posts (existing)', [post_top(*q_) for q_ in sw_p], (250, 880), OLD),
                        ('camera posts', [post_top(*c_) for c_ in cam_p], (780, 140), CAMC)])])

# (b) lid inside with the PCB in place, and the stack depth.
board_items = at(['Lid', 'ButtonCap', 'Switch', 'ControlPCB', 'StatusLED', 'J1Header', 'ControlResistors'] + CAM + ['CSIRibbon'])
pcb_labels = [
    (f'control PCB {pcb["size"][0]:g} x {pcb["size"][1]:g} x 1.6 (NEW)', (20, 93.4, pbot), (620, 925), NEW),
    ('J1 1x6 header\n(on the back)', (pcb['j1_pins'][2][0] + 1.3, pcb['j1_pins'][2][1], pcb['j1_plastic_bottom_z']), (890, 860), NEW),
    ('R1-R3 330R\n(on the back)', (9.8, C['resistor_rows_y'][1], pbot - 2.5), (890, 150), NEW),
    ('4 x M2 x 6: two existing posts, two new', [(x, y, pbot - .5) for x, y in sw_p + led_p], (560, 110), OLD),
    ('D1 leads', (lx, ly, pbot - 2), (380, 905), NEW),
    ('camera', (cx, cy - 6, lay['camera_pcb_z'][0] - 3), (140, 905), CAMC),
]
XR = 56
z_lines = [((-1, ly, H), (XR, ly, H), f'{H:g}  lid face = LED tip', ink),
           ((-1, ly, RIM), (XR, ly, RIM), f'{RIM:g}  lid underside', ink),
           ((-1, ly, lay['led_collar_z'][0]), (XR, ly, lay['led_collar_z'][0]), f'{lay["led_collar_z"][0]:g}  light collar bottom', NEW),
           ((-1, ly, lay['led_flange_z'][0]), (XR, ly, lay['led_flange_z'][0]), f'{lay["led_flange_z"][0]:g}  LED flange underside', NEW),
           ((-1, ly, seat), (XR, ly, seat), f'{seat:g}  PCB top = switch seat', OLD),
           ((-1, ly, pbot), (XR, ly, pbot), f'{pbot:g}  PCB underside', OLD),
           ((-1, ly, pcb['j1_plastic_bottom_z']), (XR, ly, pcb['j1_plastic_bottom_z']), f'{pcb["j1_plastic_bottom_z"]:g}  J1 plastic', NEW),
           ((-1, ly, pcb['dupont_housing_bottom_z']), (XR, ly, pcb['dupont_housing_bottom_z']),
            f'{pcb["dupont_housing_bottom_z"]:g}  Dupont housing end', NEW),
           ((-1, ly, 2.5), (XR, ly, 2.5), '2.5  tray floor', ink)]
section_items = at(['Tray', 'Lid', 'ButtonCap', 'Switch', 'StatusLED', 'ControlPCB', 'J1Header', 'DupontAllowance',
                    'ControlResistors', 'CameraPCB', 'CameraLens', 'CameraFrontParts', 'CameraConnector'], cut=False)
section_items = [(n + '#row', s_) for n, s_ in section_items]
annotated('pcb-lid-assembled.png', 'LID INSIDE WITH THE CONTROL PCB  +  STACK DEPTH',
          f'The PCB takes the printed carrier\'s place: same 1.6 mm, same two screws, top face {seat:g} mm = switch seat. '
          f'D1 flange {lay["led_standoff_above_pcb"]:g} mm above the board puts the dome flush with the lid face.',
          [dict(title='FROM INSIDE (mirror image of the front)', items=board_items, labels=pcb_labels,
                rect=(.02, .10, .40, .74), **inside),
           dict(title=f'SECTION THROUGH THE BUTTON / LED / LENS ROW (Y = {ly:g}), SEEN FROM THE Pi',
                items=section_items, elev=0, azim=-90, focus=(-2, 57, ly, ly, 0, h), lines=z_lines,
                rect=(.43, .10, .47, .74), px=1100,
                dims=[((lx + 4.5, ly, seat), (lx + 4.5, ly, lay['led_flange_z'][0]), f'{lay["led_standoff_above_pcb"]:g} standoff', NEW)],
                labels=[('button cap', (bx - 3, ly, H + .6), (300, 110), OLD),
                        ('switch B3F-4055', (bx - 5, ly, seat + 2), (110, 200), OLD),
                        ('D1 5 mm RGB', (lx, ly, H - 3), (610, 110), NEW),
                        ('camera', (cx, ly, lay['camera_pcb_z'][1] + 3), (900, 110), CAMC),
                        ('J1 + Dupont housing\n+ 4 mm wire bend', (C['j1_x'] + 1.4, ly, 12), (330, 870), NEW),
                        ('R1-R3 (behind)', (rx_mid := (C['resistor_x'][0] + C['resistor_x'][1]) / 2 + 2, ly, pbot - 1.2), (470, 720), NEW),
                        ('control PCB', (24, ly, pbot), (640, 640), NEW),
                        ('light collar', (lx + 3.6, ly, lay['led_collar_z'][0] + 1.5), (800, 560), NEW)])],
          size=(18, 9.5))

# (c) outside front with the new LED opening.
front = dict(elev=90, azim=-90)
ext = at(['Tray', 'Lid', 'ButtonCap', 'CameraLens'])
annotated('pcb-front-led-opening.png', 'OUTSIDE FRONT  -  new 5.5 mm LED opening',
          'The 3 mm LED bore and 2.2 mm window are replaced by a plain 5.5 mm through-hole for the 5 mm RGB LED, '
          'whose dome tip sits flush with the face. Everything else on the front is unchanged.',
          [dict(title='WHOLE FRONT', items=ext + at(['StatusLED']), focus=(-6, w + 6, -1, l + 1, 0, h), rect=(.02, .10, .26, .74),
                labels=[('LED (5 mm, flush)', (lx, ly, H), ('+', -80, 150), NEW)], **front),
           dict(title='CAMERA END, LED OUT (the opening)', items=ext, focus=(-2, w + 2, 86, l + 1, 0, h), rect=(.29, .10, .36, .74),
                labels=[('5.5 mm LED hole (NEW)', (lx + 2.0, ly - 1.9, H), (540, 870), NEW),
                        ('button cap (existing)', (bx, by - 5, H + 1.2), (230, 870), OLD),
                        ('lens (existing)', (cx, cy - 3.75, H), (820, 870), CAMC)], **front),
           dict(title='OBLIQUE, LED FITTED', items=ext + at(['StatusLED']), elev=52, azim=-72,
                focus=(-8, w + 8, 62, l + 2, 0, h + 1), rect=(.66, .10, .33, .74),
                labels=[('LED dome flush in 5.5 mm hole', (lx, ly, H), ('+', -40, -170), NEW)])],
          size=(18, 9.5))
print('PREVIEWS_OK')
