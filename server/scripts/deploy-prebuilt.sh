#!/bin/bash
# Prebuilt Vercel deploy: build on YOUR machine, upload only artifacts.
# Cloud builds recompile the whole Rust dependency tree from scratch on a
# small builder (~6-10 minutes, plus per-account queue time); this flow
# deploys in seconds. Git-push auto-builds are disabled in vercel.json
# ("git": {"deploymentEnabled": false}) — this script IS the deploy path.
#
#   scripts/deploy-prebuilt.sh             # production
#   scripts/deploy-prebuilt.sh --preview   # preview deploy
#
# One-time setup:
#   npm i -g vercel && vercel login && vercel link
#   cargo install cargo-zigbuild     # cross-compiles for Lambda's glibc
#   pip install ziglang              # zig toolchain (or install zig any way)
#
# Full guide: https://nextrs-docs.vercel.app/docs/deploy-prebuilt
set -euo pipefail
cd "$(dirname "$0")/.."

[ "${1:-}" = "--preview" ] && FLAGS=() || FLAGS=(--prod)

vercel pull --yes --environment=production > /dev/null
vercel build "${FLAGS[@]}"

# Refuse to ship if the Rust function silently failed to build (the classic
# missing-cargo-zigbuild failure: everything green, no binary in the output).
if ! find .vercel/output/functions -name '*.func' -type d 2>/dev/null | grep -q .; then
  echo "ERROR: no function in .vercel/output — is cargo-zigbuild installed and zig reachable?" >&2
  exit 1
fi

# vercel-rust records the executable under target/, which is excluded from the
# upload. Copy it into the function so the Build Output is self-contained.
python3 - <<'PY'
import json
import shutil
from pathlib import Path

for config_path in Path(".vercel/output/functions").glob("**/*.func/.vc-config.json"):
    config = json.loads(config_path.read_text())
    source = config.get("filePathMap", {}).get(config.get("handler", "executable"))
    if not source:
        continue
    source_path = Path(source)
    if not source_path.is_file():
        raise SystemExit(f"ERROR: function executable does not exist: {source_path}")
    bundled_name = config.get("handler", "executable")
    destination = config_path.parent / bundled_name
    shutil.copy2(source_path, destination)
    destination.chmod(destination.stat().st_mode | 0o111)
    config.pop("filePathMap", None)
    config_path.write_text(json.dumps(config, indent=2) + "\n")
    print(f"==> bundled {source_path} as {destination}")
PY

vercel deploy --prebuilt "${FLAGS[@]}"
