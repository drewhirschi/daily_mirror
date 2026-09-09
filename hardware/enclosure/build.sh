#!/usr/bin/env bash
set -euo pipefail
cad_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
command -v FreeCADCmd >/dev/null
FreeCADCmd "$cad_dir/build_enclosure.py"
