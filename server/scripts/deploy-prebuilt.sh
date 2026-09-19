#!/bin/bash
# Deploy the server to Vercel from locally built server bundles.
#
#   scripts/deploy-prebuilt.sh             # production
#   scripts/deploy-prebuilt.sh --preview   # preview deploy
#   scripts/deploy-prebuilt.sh --skip-cron # production, leave cron triggers alone
#
# The build runs in the Debian image (see ../scripts/native/README.md): this
# workstation's OpenCV 5 and glibc 2.44 produce binaries Vercel cannot run.
#
# Do NOT substitute `vercel build` or `nextrs deploy`. Both compile on the
# host: `vercel build` additionally knows nothing about server bundles and
# collapses all three functions into one api/index with default features,
# which drops face-inference and makes /api/processing/run answer 503.
#
# One-time setup:
#   npm i -g vercel && vercel login && vercel link
#   private vision assets present under server/resources/vision
set -euo pipefail
cd "$(dirname "$0")/.."
project_dir=$(cd .. && pwd)

PREVIEW=0; SKIP_CRON=0
for arg in "$@"; do
  case "$arg" in
    --preview) PREVIEW=1 ;;
    --skip-cron) SKIP_CRON=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

test -f .vercel/project.json || {
  echo "ERROR: server is not linked to a Vercel project; run: vercel link" >&2
  exit 1
}

# A hand-written vercel.json shadows the generated .nextrs/vercel.json and
# silently reintroduces the single-function, default-features build.
if [ -f vercel.json ]; then
  echo "ERROR: server/vercel.json exists and would shadow the generated" >&2
  echo "       .nextrs/vercel.json. Move its settings into [vercel] in" >&2
  echo "       nextrs.toml and delete the file." >&2
  exit 1
fi

# The frontend bundle is built by the host CLI: the container sets
# NEXTRS_SKIP_BUNDLE=1 and reuses public/dist. Keep this on the host.
echo "==> generating frontend and typed client"
npm run client:prepare

echo "==> building server bundles in the Debian image"
"$project_dir/scripts/native/build.sh"

# Every bundle the plan declares must have reached the output, or a route
# silently falls back to another function's feature set.
for name in $(nextrs bundles plan | python3 -c 'import json,sys; print(" ".join(json.load(sys.stdin)["bundles"]))'); do
  test -x ".vercel/output/functions/__nextrs_functions/$name.func/executable" || {
    echo "ERROR: bundle '$name' is missing from .vercel/output" >&2
    exit 1
  }
done

echo "==> deploying prebuilt output"
if [ "$PREVIEW" = "1" ]; then
  vercel deploy --prebuilt --yes
else
  vercel deploy --prebuilt --target=production --yes
  # Cloudflare-provider crons point at the production URL, which does not
  # change between deploys, so an existing trigger keeps working. Re-deploying
  # them needs Cloudflare credentials and the current CRON_SECRET.
  if [ "$SKIP_CRON" = "1" ]; then
    echo "==> skipping cron triggers (--skip-cron)"
  else
    echo "==> deploying cron triggers"
    nextrs cron deploy --root .
  fi
fi
