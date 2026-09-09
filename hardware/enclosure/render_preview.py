"""Render the actual exported FreeCAD mesh geometry; no CAD approximation."""
from pathlib import Path
import json
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from mpl_toolkits.mplot3d.art3d import Poly3DCollection
root=Path(__file__).resolve().parent/'output'
scene=json.loads((root/'scene.json').read_text())
fig=plt.figure(figsize=(15,7),facecolor='#f5f3ef')
views=[('Assembled enclosure',{'Base','Lid','CameraAdapter','CameraLens'},(0,180),(0,180),1),('Inside: Pi + wiring reserve',{'Base','PiPCB','PiCoolerKeepout','WiringReserve','CameraPCB'},(0,180),(0,180),.28),('Future rail + carriage study',{'RailChannel','CarriageMounting'},(210,310),(0,420),1)]
for i,(title,names,xlim,ylim,shellalpha) in enumerate(views):
    ax=fig.add_subplot(1,3,i+1,projection='3d',computed_zorder=False)
    ax.set_facecolor('#f5f3ef')
    for s in scene:
        if s['name'] not in names: continue
        v=np.array(s['vertices']); v=v[:,[0,2,1]];v[:,1]*=-1
        alpha=shellalpha if s['name']=='Base' else .5 if s['name'] in ['WiringReserve','PiCoolerKeepout'] else 1
        poly=Poly3DCollection(v[np.array(s['faces'])],facecolor=s['color'],edgecolor='none',alpha=alpha,rasterized=True)
        ax.add_collection3d(poly)
    ax.set_xlim(*xlim);ax.set_ylim(-100,15);ax.set_zlim(*ylim)
    ax.set_box_aspect((xlim[1]-xlim[0],115,ylim[1]-ylim[0]))
    ax.view_init(elev=18,azim=-65)
    ax.set_axis_off();ax.set_title(title,fontsize=14,pad=5)
fig.suptitle('DAILY MIRROR  /  enclosure concept 01',x=.05,ha='left',fontsize=21,weight='bold')
fig.text(.05,.07,'180 × 180 × 73 mm shell + lid\nReplaceable camera plate adds 3 mm',fontsize=12)
fig.text(.365,.07,'Green: Pi + cooler allowance\nAmber: 48 × 65 × 45 mm wiring reserve',fontsize=12)
fig.text(.69,.07,'300 mm travel • 420 mm rail\nLock and removable end stop still to design',fontsize=12)
fig.text(.05,.015,'Concept only • camera dimensions, controls, mounting loads and printer fit remain unverified',fontsize=10,color='#5a6065')
plt.subplots_adjust(left=.02,right=.98,top=.87,bottom=.15,wspace=.05)
fig.savefig(root/'enclosure-preview.png',dpi=150)
