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

def raster(items, elev, azim, width=900, height=900):
    """items: (name, (dx, dy, dz)) pairs. Per-pixel depth buffer, flat shading."""
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
    return pixels.reshape(height, SS, width, SS, 3).mean(axis=(1, 3))

def at(names, shift=(0, 0, 0), cut=False):
    return [(n + '#cut' if cut else n, shift) for n in names]

PI = ['PiPCB', 'PiPorts', 'PiHeaders', 'PiChips', 'PiSDCard']
CAM = ['CameraPCB', 'CameraLens', 'CameraFrontParts', 'CameraConnector']
w, l, h = lay['outer_mm']
head = 'DAILY MIRROR  /  Raspberry Pi 4 tray + lid'
sub = (f'{w:g} x {l:g} x {h:g} mm  /  Pi 4 Model B, Arducam IMX519 behind the lid, 12 mm tactile switch + 3 mm LED  /  '
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

BTN = ['ButtonCap', 'ButtonCarrier', 'Switch', 'StatusLED']
closed = at(['Tray', 'Lid', 'ButtonCap', 'StatusLED', 'CameraLens'])
inside = at(['Tray', 'CSIRibbon'] + PI + CAM + BTN)
cutaway = at(['Tray', 'Lid', 'CSIRibbon'] + BTN + PI + CAM, cut=True)
exploded = (at(['Tray'] + PI) + at(['CSIRibbon'], (0, 0, 12)) +
            at(['Lid', 'StatusLED'] + CAM, (0, 0, 55)) + at(['Switch', 'ButtonCarrier'], (0, 0, 40)) +
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
print('PREVIEWS_OK')
