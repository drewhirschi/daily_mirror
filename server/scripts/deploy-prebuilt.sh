#!/bin/bash
# Deploy the server to Vercel from locally built server bundles.
#
#   scripts/deploy-prebuilt.sh             # production (migrates first)
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

# `nextrs cron deploy` reads its credentials from the process environment only
# (cargo-nextrs 0.3.0: cron.rs preflight_cloudflare_credentials and deploy read
# CRON_SECRET, CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID via
# std::env::var, and nothing in the CLI loads a dotenv file). `vercel pull`
# already wrote those values to .vercel/.env.production.local, so export them
# from there rather than making every caller do it by hand. Delete this once
# nextrs loads the env file itself; see the upstream note
# docs/upstream-plans/cli-env-file-and-containerized-build.md in the nextrs
# repository.
#
# The same file also carries DAILY_MIRROR_DATABASE_URL for the migration step.
#
# Values are never printed, the file is parsed rather than sourced (its values
# are double-quoted and may contain characters that break `source`), and a
# variable already set in the environment wins over the file. Vercel writes
# "[SENSITIVE]" instead of the value for variables marked sensitive, so those
# placeholders are skipped rather than exported as if they were real.
load_env_file() {
  local file=$1 key value line
  if [ ! -f "$file" ]; then
    echo "WARNING: $file is missing; credentials must come from the" >&2
    echo "         environment. Pull them with:" >&2
    echo "         vercel env pull .vercel/.env.production.local --environment=production" >&2
    return 0
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%$'\r'}
    case "$line" in
      ''|'#'*) continue ;;
      *'='*) ;;
      *) continue ;;
    esac
    key=${line%%=*}
    value=${line#*=}
    case "$key" in
      CRON_SECRET|CLOUDFLARE_API_TOKEN|CLOUDFLARE_ACCOUNT_ID) ;;
      # The database URL is not a secret and Vercel does pull it. The token is
      # marked sensitive, so the file only ever holds a "[SENSITIVE]"
      # placeholder for it; `database_credentials` deals with that.
      DAILY_MIRROR_DATABASE_URL) ;;
      *) continue ;;
    esac
    # A variable already exported wins; the file only fills gaps.
    [ -n "${!key:-}" ] && continue
    if [ "${value#\"}" != "$value" ] && [ "${value%\"}" != "$value" ]; then
      value=${value#\"}
      value=${value%\"}
      value=${value//\\n/$'\n'}
      value=${value//\\\"/\"}
      value=${value//\\\\/\\}
    elif [ "${value#\'}" != "$value" ] && [ "${value%\'}" != "$value" ]; then
      value=${value#\'}
      value=${value%\'}
    fi
    [ -n "$value" ] || continue
    [ "$value" = "[SENSITIVE]" ] && continue
    export "$key=$value"
    echo "==> using $key from $file"
  done < "$file"
}

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

# --- Schema migrations ------------------------------------------------------
#
# Production must be at the schema version the new binaries expect BEFORE they
# start serving, so migrations run first and a failure stops the deploy. Every
# migration is expand-only (see ../docs/deployment.md), which is what makes it
# safe for the previous build to keep serving in the meantime.
#
# Credentials, in order:
#   1. DAILY_MIRROR_DATABASE_URL / DAILY_MIRROR_DATABASE_AUTH_TOKEN already in
#      the environment. A CI job or a careful operator sets both.
#   2. The URL from .vercel/.env.production.local (Vercel pulls it; it is not
#      secret) plus a one-hour token minted with the Turso CLI. Vercel does not
#      pull sensitive variables, so the file's token is a "[SENSITIVE]"
#      placeholder and cannot be used.
# Anything else stops the deploy with an explanation. The token is never
# printed and never written to disk.
turso_cli() {
  if [ -x "$HOME/.turso/turso" ]; then
    echo "$HOME/.turso/turso"
  elif command -v turso >/dev/null 2>&1; then
    command -v turso
  fi
}

# libsql://daily-mirror-someorg.turso.io -> daily-mirror
turso_database_name() {
  local host=${1#*://}
  host=${host%%/*}
  host=${host%%.*}
  # The host label is "<database>-<organization>"; drop the last segment.
  echo "${host%-*}"
}

database_credentials() {
  if [ -z "${DAILY_MIRROR_DATABASE_URL:-}" ]; then
    load_env_file .vercel/.env.production.local
  fi
  if [ -z "${DAILY_MIRROR_DATABASE_URL:-}" ]; then
    echo "ERROR: DAILY_MIRROR_DATABASE_URL is not set and could not be read" >&2
    echo "       from .vercel/.env.production.local. Pull it with:" >&2
    echo "       vercel env pull .vercel/.env.production.local --environment=production" >&2
    return 1
  fi
  if [ -n "${DAILY_MIRROR_DATABASE_AUTH_TOKEN:-}" ]; then
    echo "==> using the database token from the environment"
    return 0
  fi
  local turso
  turso=$(turso_cli)
  if [ -z "$turso" ]; then
    echo "ERROR: no database token." >&2
    echo "       Vercel does not pull sensitive variables, so" >&2
    echo "       .vercel/.env.production.local cannot supply it. Either export" >&2
    echo "       DAILY_MIRROR_DATABASE_AUTH_TOKEN yourself, or install the" >&2
    echo "       Turso CLI so this script can mint a one-hour token:" >&2
    echo "       curl -sSfL https://get.tur.so/install.sh | bash" >&2
    return 1
  fi
  local name=${DAILY_MIRROR_TURSO_DB:-}
  [ -n "$name" ] || name=$(turso_database_name "$DAILY_MIRROR_DATABASE_URL")
  echo "==> minting a one-hour token for Turso database '$name'"
  local token
  # Captured, never echoed. A failure here prints the CLI's own message.
  token=$("$turso" db tokens create "$name" --expiration 1h) || {
    echo "ERROR: could not mint a Turso token for '$name'." >&2
    echo "       Run '$turso auth login', or set DAILY_MIRROR_TURSO_DB if the" >&2
    echo "       database is not named after the URL host." >&2
    return 1
  }
  [ -n "$token" ] || { echo "ERROR: the Turso CLI returned an empty token" >&2; return 1; }
  export DAILY_MIRROR_DATABASE_AUTH_TOKEN="$token"
}

migrate_production() {
  database_credentials || return 1
  echo "==> checking the production schema"
  cargo run --locked --quiet --bin daily-mirror-migrate -- status || return 1
  echo "==> applying pending migrations to production"
  cargo run --locked --quiet --bin daily-mirror-migrate -- up || return 1
}

echo "==> deploying prebuilt output"
if [ "$PREVIEW" = "1" ]; then
  vercel deploy --prebuilt --yes
else
  # Before the new build serves a single request.
  migrate_production || {
    echo "ERROR: migrations did not complete; nothing was deployed." >&2
    exit 1
  }
  vercel deploy --prebuilt --target=production --yes
  # Cloudflare-provider crons point at the production URL, which does not
  # change between deploys, so an existing trigger keeps working. Re-deploying
  # them needs Cloudflare credentials and the current CRON_SECRET.
  if [ "$SKIP_CRON" = "1" ]; then
    echo "==> skipping cron triggers (--skip-cron)"
  else
    echo "==> deploying cron triggers"
    load_env_file .vercel/.env.production.local
    nextrs cron deploy --root .
  fi
fi
