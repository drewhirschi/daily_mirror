#!/usr/bin/env bash
# Slice the printable parts for the Bambu A1 (0.20 mm Standard, PLA Basic, 4 walls,
# 20% infill, textured PEI, bed 55 C). The Tray + Lid + button parts plate goes to output/ and the
# single-part files to sliced/ (git-ignored).
# This only slices; nothing is sent to the printer.
set -euo pipefail
cad_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
slicer="$cad_dir/../../tools/bambu/slice.py"
mkdir -p "$cad_dir/sliced"
for part in PiFitCoupon LidFitCoupon ButtonCap ButtonCarrier Tray Lid; do
  /usr/bin/python3 "$slicer" "$cad_dir/sliced/$part.gcode.3mf" "$cad_dir/output/$part.stl" --bed-temp 55
done
/usr/bin/python3 "$slicer" "$cad_dir/output/TrayAndLid.gcode.3mf" "$cad_dir/output/Tray.stl" "$cad_dir/output/Lid.stl" \
  "$cad_dir/output/ButtonCap.stl" "$cad_dir/output/ButtonCarrier.stl" --bed-temp 55
