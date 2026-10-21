#!/usr/bin/env bash
# Slice the printable parts for the Bambu A1 (0.20 mm Standard, PLA Basic, 4 walls,
# 20% infill, textured PEI, bed 55 C). Output goes to sliced/ (git-ignored).
# This only slices; nothing is sent to the printer.
set -euo pipefail
cad_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
slicer="$cad_dir/../../tools/bambu/slice.py"
mkdir -p "$cad_dir/sliced"
for part in PerfboardFitCoupon Tray CameraLid; do
  /usr/bin/python3 "$slicer" "$cad_dir/sliced/$part.gcode.3mf" "$cad_dir/output/$part.stl" --bed-temp 55
done
