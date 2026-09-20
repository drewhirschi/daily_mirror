#!/usr/bin/env python3
"""Build an IMX519 contrast-autofocus tuning candidate on a Pi.

Uses the installed sensor calibration plus the installed AF algorithm settings.
The 0..4095 actuator map is a starting point, not calibrated subject distance.
Reference: https://lists.libcamera.org/pipermail/libcamera-devel/2023-June/038295.html
Test against the actual module before installing. Stock files remain unchanged.
"""
import argparse
import json
from pathlib import Path
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("output", type=Path)
parser.add_argument("--shadow-exponent", type=float, default=1.0,
                    help="Output gamma exponent; 0.85 gently lifts shadows, 1 keeps stock")
args = parser.parse_args()
if not 0.7 <= args.shadow_exponent <= 1.0:
    parser.error("shadow exponent must be between 0.7 and 1.0")
base=Path('/usr/share/libcamera/ipa/rpi/vc4')
j=json.loads((base/'imx519.json').read_text())
af=next(a for a in json.loads((base/'imx708.json').read_text())['algorithms'] if 'rpi.af' in a)
af['rpi.af']['map']=[0,0,15,4095]
for speed in af['rpi.af']['speeds'].values():speed['pdaf_frames']=0
j['algorithms'].append(af)
for algorithm in j['algorithms']:
    if 'rpi.contrast' in algorithm:
        curve = algorithm['rpi.contrast']['gamma_curve']
        for i in range(1, len(curve), 2):
            curve[i] = round(65535 * (curve[i] / 65535) ** args.shadow_exponent)
args.output.write_text(json.dumps(j,indent=2))
