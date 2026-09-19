"""Produce illustrated previews and a self-contained viewer from actual CAD meshes."""
from pathlib import Path
import json
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

ROOT=Path(__file__).resolve().parent
OUT=ROOT/'output'
scene=json.loads((OUT/'scene.json').read_text())
bg='#eeede7'
ink='#243b44'

def draw(ax,names,elev,azim,limits=None):
    triangles=[]
    colors=[]
    yaw,pitch=np.radians([-(azim+90),elev])
    cy,sy,cp,sp=np.cos(yaw),np.sin(yaw),np.cos(pitch),np.sin(pitch)
    rotation=np.array([[cy,0,sy],[sp*sy,cp,-sp*cy],[-cp*sy,sp,cp*cy]])
    for s in scene:
        if s['name'] not in names: continue
        v=np.array(s['vertices'])
        tris=v[np.array(s['faces'])]
        normal=np.cross(tris[:,1]-tris[:,0],tris[:,2]-tris[:,0])
        normal/=np.maximum(np.linalg.norm(normal,axis=1)[:,None],1e-8)
        light=np.array([-.35,.65,1]);light/=np.linalg.norm(light)
        intensity=.48+.52*np.maximum(0,normal@light)
        colors.extend(np.clip(np.array(s['color'])[None,:]*intensity[:,None],0,1))
        triangles.extend(tris@rotation.T)
    tris=np.array(triangles);colors=np.array(colors)
    lo=tris.min(axis=(0,1));hi=tris.max(axis=(0,1))
    width,height=850,900
    scale=min((width-70)/(hi[0]-lo[0]),(height-60)/(hi[1]-lo[1]))
    tris[:,:,0]=(tris[:,:,0]-(hi[0]+lo[0])/2)*scale+width/2
    tris[:,:,1]=height/2-(tris[:,:,1]-(hi[1]+lo[1])/2)*scale
    pixels=np.ones((height,width,4),dtype=np.float32)
    pixels[:,:,:3]=matplotlib.colors.to_rgb(bg)
    depth=np.full((height,width),-np.inf)
    # A per-pixel depth buffer correctly resolves the concave enclosure and holes.
    for tri,color in zip(tris,colors):
        x0,y0=np.maximum(np.floor(tri[:,:2].min(0)).astype(int),0)
        x1,y1=np.minimum(np.ceil(tri[:,:2].max(0)).astype(int),[width-1,height-1])
        if x1<x0 or y1<y0:continue
        a,b,c=tri
        den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
        if abs(den)<1e-8:continue
        xx=np.arange(x0,x1+1)[None,:]+.5;yy=np.arange(y0,y1+1)[:,None]+.5
        wa=((b[1]-c[1])*(xx-c[0])+(c[0]-b[0])*(yy-c[1]))/den
        wb=((c[1]-a[1])*(xx-c[0])+(a[0]-c[0])*(yy-c[1]))/den
        wc=1-wa-wb
        zz=wa*a[2]+wb*b[2]+wc*c[2]
        target=depth[y0:y1+1,x0:x1+1]
        mask=(wa>=-1e-6)&(wb>=-1e-6)&(wc>=-1e-6)&(zz>target)
        target[mask]=zz[mask]
        pixels[y0:y1+1,x0:x1+1,:3][mask]=color
    ax.imshow(pixels)
    ax.set_axis_off();ax.set_facecolor(bg)

fig=plt.figure(figsize=(16,9),facecolor=bg)
views=[
 ('01   ENCLOSURE',{'Housing','FrontPanel','ButtonCap','LEDAllowance','CameraLensAllowance','LockKnob'},18,-65),
 ('02   INTERNAL MOUNTS',{'Housing','Pi4PCB','CameraPCB','CameraLensAllowance','PowerPlugAllowance'},42,-65),
 ('03   ADJUSTABLE RAIL',{'RailLower','RailUpper','Carriage','StopLower','StopUpper','LockKnob','LockScrew'},20,-60)]
for i,(title,names,elev,azim) in enumerate(views):
    ax=fig.add_subplot(1,3,i+1)
    draw(ax,names,elev,azim)
    ax.set_title(title,loc='left',fontsize=11,fontweight='bold',color=ink,pad=0)
fig.text(.045,.935,'DAILY MIRROR',size=13,weight='bold',color=ink)
fig.text(.045,.875,'A compact camera enclosure. An adjustable home.',size=25,weight='bold',color=ink)
fig.text(.045,.825,'Raspberry Pi 4 B  /  Standard PiCam mounts  /  Prototype 02',size=12,color=ink)
fig.text(.055,.135,'140 × 170 × 55 mm\nLens, status LED, capture button',size=11,color=ink,linespacing=1.7)
fig.text(.377,.135,'Raised Pi mounts + power tie saddles\nCamera screws into front-panel posts',size=11,color=ink,linespacing=1.7)
fig.text(.70,.135,'300 mm travel / two rail sections\nAccessible thumb lock + removable stops',size=11,color=ink,linespacing=1.7)
fig.text(.045,.055,'Rendered from the exported CAD geometry. Electronics are reference envelopes. Physical fit and holding load remain untested.',size=10,color=ink)
plt.subplots_adjust(left=.035,right=.97,top=.77,bottom=.20,wspace=.02)
fig.savefig(OUT/'enclosure-preview.png',dpi=150,facecolor=bg)
plt.close(fig)

fig=plt.figure(figsize=(14,7),facecolor=bg)
ax=fig.add_subplot(121)
draw(ax,{'FrontPanel'},30,60)
ax.set_title('INSIDE THE FRONT PANEL',fontsize=13,color=ink,loc='left')
ax=fig.add_subplot(122)
draw(ax,{'Carriage','LockKnob','LockNut','LockScrew'},25,-60)
ax.set_title('CARRIAGE + THUMB LOCK',fontsize=13,color=ink,loc='left')
fig.text(.06,.06,'Four Ø1.6 mm pilot holes on a 21 × 12.5 mm pattern\n9 mm camera posts; LED light baffle',size=12,color=ink,linespacing=1.7)
fig.text(.56,.06,'Captive M4 nut, M4 × 16 screw, rubber/TPU brake pad\nLoosen to slide; tighten gently to hold position',size=12,color=ink,linespacing=1.7)
plt.subplots_adjust(left=.04,right=.97,top=.91,bottom=.20,wspace=.1)
fig.savefig(OUT/'mount-details.png',dpi=150,facecolor=bg)
plt.close(fig)

compact=[]
for s in scene:
    if s['kind']=='coupon':continue
    compact.append({k:([[round(n,3) for n in p] for p in value] if k=='vertices' else value)
                    for k,value in s.items() if k in ('name','kind','vertices','faces','color')})
html=(ROOT/'viewer-template.html').read_text().replace('/*__SCENE__*/',json.dumps(compact,separators=(',',':')))
(OUT/'model-viewer.html').write_text(html)
print('PREVIEWS_OK')
