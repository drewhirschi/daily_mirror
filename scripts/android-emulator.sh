#!/usr/bin/env bash
# Drive the Android emulator for the Expo app in mobile/.
# See docs/android-emulator.md for prerequisites and one-time SDK setup.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export JAVA_HOME="${JAVA_HOME_ANDROID:-$HOME/.local/share/jdk17}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.android/avd}"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"

AVD="${AVD:-daily_mirror}"
APP_ID="app.dailymirror.android"
SCHEME="dailymirror"
METRO_PORT=8081

die() { echo "error: $*" >&2; exit 1; }

require_device() {
  adb get-state >/dev/null 2>&1 || die "no emulator running; run '$0 boot' first"
}

cmd_boot() {
  if adb get-state >/dev/null 2>&1; then
    echo "emulator already running"
    return 0
  fi
  emulator -list-avds | grep -qx "$AVD" || die "AVD '$AVD' not found (checked \$ANDROID_AVD_HOME=$ANDROID_AVD_HOME)"
  local start; start=$(date +%s)
  echo "booting $AVD headlessly..."
  nohup emulator -avd "$AVD" -no-window -no-audio \
    -gpu swiftshader_indirect -no-boot-anim >/tmp/android-emulator-"$AVD".log 2>&1 &
  adb wait-for-device
  while [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" != "1" ]; do
    sleep 2
  done
  echo "boot_completed in $(( $(date +%s) - start ))s"
  adb devices
}

cmd_install() {
  require_device
  [ -d "$REPO_ROOT/mobile/android" ] || \
    die "mobile/android is missing; run: cd mobile && npx expo prebuild --platform android"
  ( cd "$REPO_ROOT/mobile/android" && ./gradlew assembleDebug )
  adb install -r "$REPO_ROOT/mobile/android/app/build/outputs/apk/debug/app-debug.apk"
}

cmd_app() {
  require_device
  adb reverse "tcp:$METRO_PORT" "tcp:$METRO_PORT"
  if ! curl -s "http://127.0.0.1:$METRO_PORT/status" 2>/dev/null | grep -q running; then
    echo "starting Metro..."
    # setsid + disown so Metro fully detaches and this script does not wait on it
    setsid bash -c "cd '$REPO_ROOT' && exec env EXPO_NO_TELEMETRY=1 npm run mobile" \
      >/tmp/android-metro.log 2>&1 </dev/null &
    disown %% 2>/dev/null || true
    for _ in $(seq 1 60); do
      curl -s "http://127.0.0.1:$METRO_PORT/status" 2>/dev/null | grep -q running && break
      sleep 2
    done
  fi
  curl -s "http://127.0.0.1:$METRO_PORT/status" | grep -q running \
    || die "Metro did not come up; see /tmp/android-metro.log"
  echo "Metro is running (log: /tmp/android-metro.log)"
  adb shell am start -a android.intent.action.VIEW \
    -d "$SCHEME://expo-development-client/?url=http%3A%2F%2Flocalhost%3A$METRO_PORT"
  echo "launched $APP_ID; the first bundle takes a few seconds"
}

cmd_record() {
  local secs="${1:-}" out="${2:-}"
  [ -n "$secs" ] && [ -n "$out" ] || die "usage: $0 record <seconds> <out.mp4>"
  require_device
  adb shell screenrecord --time-limit "$secs" /sdcard/_rec.mp4
  adb pull /sdcard/_rec.mp4 "$out"
  adb shell rm -f /sdcard/_rec.mp4
  echo "wrote $out"
}

cmd_screenshot() {
  local out="${1:-}"
  [ -n "$out" ] || die "usage: $0 screenshot <out.png>"
  require_device
  adb exec-out screencap -p > "$out"
  echo "wrote $out"
}

cmd_stop() {
  adb emu kill 2>/dev/null || true
  pkill -f 'expo start' 2>/dev/null || true
  # adb emu kill returns before qemu actually exits; wait it out
  for _ in $(seq 1 30); do
    pgrep -f 'qemu-system-x86_64-headless' >/dev/null || break
    sleep 1
  done
  if pgrep -f 'qemu-system-x86_64-headless' >/dev/null; then
    echo "warning: emulator process still alive" >&2
  else
    echo "stopped emulator and Metro"
  fi
}

case "${1:-}" in
  boot)       cmd_boot ;;
  install)    cmd_install ;;
  app)        cmd_app ;;
  record)     shift; cmd_record "$@" ;;
  screenshot) shift; cmd_screenshot "$@" ;;
  stop)       cmd_stop ;;
  *)
    cat <<EOF
usage: $0 <command>

  boot                    boot the '$AVD' AVD headlessly and wait for boot_completed
  install                 gradlew assembleDebug, then adb install the debug dev client
  app                     adb reverse, start Metro if needed, deep-link the dev client
  screenshot <out.png>    capture the screen
  record <secs> <out.mp4> record the screen and pull the clip
  stop                    kill the emulator and Metro

See docs/android-emulator.md.
EOF
    exit 1 ;;
esac
