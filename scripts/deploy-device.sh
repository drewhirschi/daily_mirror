#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."

PI_HOST="${DAILY_MIRROR_PI_HOST:-drew@rpi1.local}"
PI_ADMIN_URL="${DAILY_MIRROR_PI_ADMIN_URL:-http://rpi1.local:8081}"
BINARY="device/target/aarch64-unknown-linux-gnu/release/daily-mirror-device"
REMOTE_DIR="/home/drew/daily-mirror-device/bin"
REMOTE_BINARY="$REMOTE_DIR/daily-mirror-device"
REMOTE_NEXT="$REMOTE_BINARY.next"
REMOTE_PREVIOUS="$REMOTE_BINARY.previous"
SSH=(ssh -4 -F /dev/null -o BatchMode=yes)
SCP=(scp -4 -F /dev/null -o BatchMode=yes)

if [ ! -x "$BINARY" ]; then
  echo "ERROR: missing release binary; run 'just device-build' first" >&2
  exit 1
fi

"${SCP[@]}" "$BINARY" "$PI_HOST:$REMOTE_NEXT"
"${SSH[@]}" "$PI_HOST" "set -e
if test -x '$REMOTE_BINARY'; then
  sudo cp --preserve=mode,ownership,timestamps '$REMOTE_BINARY' '$REMOTE_PREVIOUS'
fi
sudo chown root:root '$REMOTE_NEXT'
sudo chmod 0755 '$REMOTE_NEXT'
sudo mv -f '$REMOTE_NEXT' '$REMOTE_BINARY'
sudo systemctl restart daily-mirror-device.service"

for _attempt in $(seq 1 20); do
  if curl --fail --silent --show-error --max-time 2 "$PI_ADMIN_URL/healthz"; then
    echo
    echo "Pi deploy is healthy"
    exit 0
  fi
  sleep 0.5
done

echo "ERROR: Pi health check failed; restoring the previous binary" >&2
"${SSH[@]}" "$PI_HOST" "set -e
test -x '$REMOTE_PREVIOUS'
sudo cp --preserve=mode,ownership,timestamps '$REMOTE_PREVIOUS' '$REMOTE_NEXT'
sudo mv -f '$REMOTE_NEXT' '$REMOTE_BINARY'
sudo systemctl restart daily-mirror-device.service"
echo "Previous binary restored; inspect with 'just pi-status' and 'just pi-logs'" >&2
exit 1
