#!/usr/bin/env python3
"""Cycle the P4's status LED red, green, blue over the USB serial console.

    tools/p4/led-check.py [--port /dev/ttyACM0] [--hold 4]

Opening the port resets the board, so this waits for it to boot first.
Needs the firmware's `led r g b` serial command (main/mirror_serial.c).
"""
import argparse
import time
import serial

ap = argparse.ArgumentParser()
ap.add_argument('--port', default='/dev/ttyACM0')
ap.add_argument('--hold', type=float, default=4.0, help='seconds per colour')
args = ap.parse_args()

s = serial.Serial(args.port, 2000000, timeout=0.2)
print('waiting for the board to boot...')
time.sleep(9)
s.write(b'\n')
time.sleep(0.3)
s.read(1 << 20)
for name, rgb in (('red', '255 0 0'), ('green', '0 255 0'), ('blue', '0 0 255')):
    s.write(f'led {rgb}\n'.encode())
    time.sleep(0.3)
    ok = any('mirror_serial: led' in l for l in s.read(8192).decode(errors='ignore').splitlines())
    print(f'{name:6} {"sent" if ok else "NOT acknowledged"}')
    time.sleep(args.hold)
s.write(b'led off\n')
print('back to normal patterns')
