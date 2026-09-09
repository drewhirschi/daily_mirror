#!/usr/bin/env bash
set -euo pipefail

# Use a dedicated checkout: no .env files, captures, keys, or server data are synced.
mac_host="${DAILY_MIRROR_MAC_HOST:-JSN}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ssh_options=(-F /dev/null -o BatchMode=yes -o ConnectTimeout=10)
# Noninteractive SSH does not load the Mac's interactive shell configuration.
remote_setup='set -e
export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export LANG=en_US.UTF-8
if [ -z "${DEVELOPER_DIR:-}" ] && [ "$(xcode-select -p)" = /Library/Developer/CommandLineTools ] && [ -d /Applications/Xcode.app/Contents/Developer ]; then
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi'


case "${1:-doctor}" in
  doctor)
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'; hostname; sw_vers; xcodebuild -version; node --version; npm --version; xcrun simctl list devices booted'
    ;;
  sync)
    ssh "${ssh_options[@]}" "$mac_host" 'mkdir -p "$HOME/work/daily-mirror-mobile"'
    rsync -az -e 'ssh -F /dev/null -o BatchMode=yes -o ConnectTimeout=10' \
      --exclude=node_modules --exclude=.expo --exclude=dist --exclude=ios --exclude=android \
      --exclude='.env' --exclude='.env.*' \
      "$repo_root/package.json" "$repo_root/package-lock.json" "$repo_root/mobile" "$repo_root/packages" \
      "$mac_host:work/daily-mirror-mobile/"
    ;;
  build)
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'; cd "$HOME/work/daily-mirror-mobile"; npm ci; cd mobile; npx expo run:ios --no-bundler'
    ;;
  archive)
    # A standalone Release app embeds its JavaScript and does not need Metro.
    # Automatic provisioning requires an Apple account in Xcode on this host.
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'; cd "$HOME/work/daily-mirror-mobile/mobile/ios"; xcodebuild -workspace DailyMirror.xcworkspace -scheme DailyMirror -configuration Release -destination "generic/platform=iOS" -archivePath ../../DailyMirror.xcarchive -derivedDataPath ../../build-jsn -allowProvisioningUpdates DEVELOPMENT_TEAM=C9P58ZP4AQ archive'
    ;;
  start)
    ssh -t -F /dev/null "$mac_host" "$remote_setup"'; cd "$HOME/work/daily-mirror-mobile"; npm run mobile'
    ;;
  *) echo 'Usage: scripts/mobile-mac.sh {doctor|sync|build|archive|start}' >&2; exit 2 ;;
esac
