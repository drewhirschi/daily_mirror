#!/usr/bin/env bash
set -euo pipefail
cad_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cad_runtime="$(mktemp -d /tmp/daily-mirror-freecad.XXXXXX)"
trap 'rm -rf -- "$cad_runtime"' EXIT
XDG_CACHE_HOME="$cad_runtime" FreeCADCmd -u "$cad_runtime/user.cfg" -s "$cad_runtime/system.cfg" "$cad_dir/build.py"
XDG_CACHE_HOME="$cad_runtime" FreeCADCmd -u "$cad_runtime/user.cfg" -s "$cad_runtime/system.cfg" "$cad_dir/verify.py"
MPLCONFIGDIR=/tmp/daily-mirror-matplotlib python "$cad_dir/render.py"
python "$cad_dir/package.py"
