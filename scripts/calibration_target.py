#!/usr/bin/env python3
"""Display camera targets. Write chart, text, gray, or image:/absolute/path
into /tmp/mirror-target-mode to change the existing window. Escape closes it.
Requires tkinter and Pillow. Move the window with the desktop compositor.
"""
import tkinter as tk
from pathlib import Path
from PIL import Image, ImageTk
r=tk.Tk();r.title('Daily Mirror Calibration');r.geometry('1080x1920')
c=tk.Canvas(r,bg='white',highlightthickness=0);c.pack(fill='both',expand=True)
last=None
def draw():
 global last
 mode=Path('/tmp/mirror-target-mode').read_text().strip() if Path('/tmp/mirror-target-mode').exists() else 'chart'
 key=(mode,c.winfo_width(),c.winfo_height())
 if key!=last:
  last=key;c.delete('all');w,h=key[1:];c.configure(bg='#808080' if mode=='gray' else 'white')
  if mode=='chart':
   c.create_rectangle(12,12,w-12,h-12,width=12)
   c.create_text(w/2,90,text='DAILY MIRROR\nIMX519 • FOCUS + COLOR',font=('DejaVu Sans',32,'bold'),justify='center')
   for i,col in enumerate(['#fff','#ffff00','#00ffff','#00ff00','#ff00ff','#ff0000','#0000ff','#000']):
    c.create_rectangle(40+i*(w-80)/8,190,40+(i+1)*(w-80)/8,420,fill=col,outline='')
   for i in range(11):
    v=int(i*255/10);c.create_rectangle(40+i*(w-80)/11,440,40+(i+1)*(w-80)/11,570,fill=f'#{v:02x}{v:02x}{v:02x}',outline='')
   y=640
   for size in [40,32,24,18,14,10]:
    c.create_text(w/2,y,text=f'{size} pt • FOCUS 0123456789 • ABC xyz',font=('DejaVu Sans',size));y+=90
   for row in range(12):
    for col in range(16):
     c.create_rectangle(40+col*(w-80)/16,1220+row*40,40+(col+1)*(w-80)/16,1260+row*40,fill='black' if (row+col)%2 else 'white',outline='')
   c.create_text(w/2,h-90,text='TOP ↑    LEFT ←    → RIGHT\nCalibration target • Escape closes',font=('DejaVu Sans',23),justify='center')
  elif mode.startswith('image:'):
   try:
    im=Image.open(mode[6:]);im.thumbnail((w-60,h-160));r.target_image=ImageTk.PhotoImage(im)
    c.create_image(w/2,h/2,image=r.target_image)
    c.create_text(w/2,70,text='IMAGE TARGET',font=('DejaVu Sans',32,'bold'))
   except (OSError,ValueError) as e:
    c.create_text(w/2,h/2,text=str(e),width=w-80)
  elif mode=='text':
   for i,size in enumerate([48,36,28,20,14]):c.create_text(w/2,250+i*240,text=f'Daily Mirror\n{size} pt  ABC 123 xyz',font=('DejaVu Sans',size),justify='center')
 r.after(250,draw)
r.bind('<Escape>',lambda e:r.destroy());r.after(200,draw);r.mainloop()
