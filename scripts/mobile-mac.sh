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
    # React Native and Expo ship separate Debug and Release builds of their
    # prebuilt binaries, and both projects' "pick the right one" script phases
    # detect Debug only by looking for DEBUG=1 in GCC_PREPROCESSOR_DEFINITIONS.
    # CocoaPods never defines that for pod targets, so a Debug build silently
    # links the Release React/ExpoModulesCore frameworks. The app then fails to
    # link (RCTPackagerConnection, react::Sealable, ShadowNode::getDebugName)
    # or, once linked, segfaults in react::Props::Props() because debug and
    # release Props have different layouts. Passing DEBUG=1 fixes the detection.
    # The swap is also skipped outright when no marker file exists, because it
    # assumes an unmarked checkout is already Debug -- pod install actually
    # lays down the Release artifacts, so record that before building.
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'
      cd "$HOME/work/daily-mirror-mobile"
      npm ci
      cd mobile
      npx pod-install
      cd ios
      printf Release > Pods/React-Core-prebuilt/.last_build_configuration
      printf Release > Pods/ReactNativeDependencies/.last_build_configuration
      for marker in Pods/*/artifacts/.last_build_configuration; do
        [ -e "$marker" ] && printf release > "$marker"
      done
      xcodebuild -workspace DailyMirror.xcworkspace -scheme DailyMirror \
        -configuration Debug -destination "generic/platform=iOS Simulator" \
        -derivedDataPath ./build \
        GCC_PREPROCESSOR_DEFINITIONS="\$(inherited) DEBUG=1" build'
    ;;
  archive)
    # A standalone Release app embeds its JavaScript and does not need Metro.
    # Automatic provisioning requires an Apple account in Xcode on this host.
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'; cd "$HOME/work/daily-mirror-mobile/mobile/ios"; xcodebuild -workspace DailyMirror.xcworkspace -scheme DailyMirror -configuration Release -destination "generic/platform=iOS" -archivePath ../../DailyMirror.xcarchive -derivedDataPath ../../build-jsn -allowProvisioningUpdates DEVELOPMENT_TEAM=C9P58ZP4AQ archive'
    ;;
  store)
    # The App Store build: prebuild, archive and export an .ipa signed for
    # distribution, ready to hand to Xcode Organizer or Transporter. Nothing
    # here uploads anything, and no credential is read or written.
    #
    # xcodebuild runs as a one-off LaunchAgent in the GUI domain because
    # automatic signing talks to the Xcode account stored in the login
    # keychain, which an ssh session cannot unlock. KeepAlive=false matters:
    # `launchctl submit` respawns the job and two concurrent xcodebuilds
    # deadlock on "build.db is locked".
    ssh "${ssh_options[@]}" "$mac_host" "$remote_setup"'
      set -e
      cd "$HOME/work/daily-mirror-mobile"
      npm ci
      cd mobile
      npx expo prebuild -p ios --clean
      root="$HOME/work/daily-mirror-mobile"
      rm -rf "$root/DailyMirror.xcarchive" "$root/export"
      cat > "$root/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>export</string>
  <key>teamID</key><string>C9P58ZP4AQ</string>
  <key>signingStyle</key><string>automatic</string>
  <key>uploadSymbols</key><true/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict>
</plist>
PLIST
      cat > "$root/store-build.sh" <<SCRIPT
#!/bin/bash
set -x
export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export LANG=en_US.UTF-8
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
cd "$root/mobile/ios"
xcodebuild -workspace DailyMirror.xcworkspace -scheme DailyMirror \
  -configuration Release -destination "generic/platform=iOS" \
  -archivePath "$root/DailyMirror.xcarchive" -derivedDataPath "$root/build-jsn" \
  -allowProvisioningUpdates DEVELOPMENT_TEAM=C9P58ZP4AQ archive
xcodebuild -exportArchive -archivePath "$root/DailyMirror.xcarchive" \
  -exportOptionsPlist "$root/ExportOptions.plist" -exportPath "$root/export" \
  -allowProvisioningUpdates
SCRIPT
      chmod +x "$root/store-build.sh"
      cat > "$root/store-build.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.dailymirror.storebuild</string>
  <key>ProgramArguments</key><array><string>$root/store-build.sh</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>StandardOutPath</key><string>$root/store-build.log</string>
  <key>StandardErrorPath</key><string>$root/store-build.log</string>
</dict>
</plist>
PLIST
      : > "$root/store-build.log"
      launchctl bootout "gui/$(id -u)/app.dailymirror.storebuild" 2>/dev/null || true
      launchctl bootstrap "gui/$(id -u)" "$root/store-build.plist"
      while launchctl print "gui/$(id -u)/app.dailymirror.storebuild" >/dev/null 2>&1; do sleep 20; done
      tail -40 "$root/store-build.log"
      ls -l "$root/export" 2>/dev/null || echo "NO EXPORT DIRECTORY"'
    ;;
  start)
    ssh -t -F /dev/null "$mac_host" "$remote_setup"'; cd "$HOME/work/daily-mirror-mobile"; npm run mobile'
    ;;
  *) echo 'Usage: scripts/mobile-mac.sh {doctor|sync|build|archive|store|start}' >&2; exit 2 ;;
esac
