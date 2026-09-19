#!/bin/bash
# Deploy the server to Vercel via nextrs, the bundle-aware build.
#
#   scripts/deploy-prebuilt.sh             # production
#   scripts/deploy-prebuilt.sh --preview   # preview deploy
#   scripts/deploy-prebuilt.sh --skip-cron # skip Cloudflare cron triggers
#
# `nextrs deploy` is the whole ship path: it regenerates .nextrs/vercel.json
# from nextrs.toml, compiles one function per bundle in nextrs.toml with that
# bundle's cargo features, uploads the prebuilt output, then deploys the
# Cloudflare cron triggers. Git-push auto-builds stay disabled — this script
# IS the deploy path.
#
# Do NOT call `vercel build` here. It knows nothing about server bundles and
# compiles a single api/index.rs with default features only, which silently
# drops face-inference and makes /api/processing/run return 503 forever.
#
# One-time setup:
#   npm i -g vercel && vercel login && vercel link
#   cargo install cargo-nextrs cargo-zigbuild   # zigbuild targets Lambda glibc
#   pip install ziglang                         # zig toolchain
#   ../scripts/native/build.sh                  # builds resources/vision
#
# Full guide: https://nextrs-docs.vercel.app/docs/deploy-prebuilt
set -euo pipefail
cd "$(dirname "$0")/.."

command -v nextrs >/dev/null || {
  echo "ERROR: cargo-nextrs is not installed; run: cargo install cargo-nextrs" >&2
  exit 1
}

test -f .vercel/project.json || {
  echo "ERROR: server is not linked to a Vercel project; run: vercel link" >&2
  exit 1
}

# A hand-written vercel.json shadows the generated .nextrs/vercel.json and
# silently reintroduces the single-function, default-features build. The
# generated file is the only supported config while bundles are in use.
if [ -f vercel.json ]; then
  echo "ERROR: server/vercel.json exists and would shadow the generated" >&2
  echo "       .nextrs/vercel.json. Move its settings into [vercel] in" >&2
  echo "       nextrs.toml and delete the file." >&2
  exit 1
fi

# Every bundle needs its declared assets on disk; resources/vision is
# gitignored and built separately, so a fresh clone fails here, not in prod.
missing=$(nextrs bundles plan \
  | python3 -c 'import json,sys,os
plan = json.load(sys.stdin)
print(" ".join(
    a for b in plan["bundles"].values() for a in b["assets"] if not os.path.exists(a)
))')
if [ -n "$missing" ]; then
  echo "ERROR: bundle assets are missing: $missing" >&2
  echo "       Build them first (see scripts/native/build.sh)." >&2
  exit 1
fi

exec nextrs deploy "$@"
