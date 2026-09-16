# Android emulator for the Expo app (Linux)

How to run the Expo app in `mobile/` on a headless Android emulator on this
Linux workstation. Every command below was run successfully on Arch Linux with
20 cores, 31 GB RAM, and a world-accessible `/dev/kvm`. All commands run from
the repository root unless stated otherwise.

`scripts/android-emulator.sh` wraps everything here; see the bottom of this
document.

## Prerequisites

- Hardware virtualisation: `/dev/kvm` must be readable and writable by your
  user (`ls -l /dev/kvm`). Without it the emulator falls back to software
  emulation and is unusably slow.
- Node 22 and npm (this repo is a root npm workspace containing `mobile/`).
- `unzip`, `curl`, and `ffmpeg` (`/usr/bin/ffmpeg`) for the recording workflow.
- Roughly 17 GB of free disk: SDK 7.1 GB, AVD 1.7 GB, JDK 17 318 MB,
  Gradle caches 4.3 GB, `mobile/android` build output 3.3 GB.

### JDK 17, not the system JDK

The system JDK on this machine is Java 26 (`/usr/bin/java`). The Android
Gradle Plugin used by Expo SDK 57 / React Native 0.86 does not support a JDK
that new, so a Temurin 17 JDK is installed **without** pacman or sudo:

```sh
mkdir -p ~/.local/share/jdk17
curl -sL -o /tmp/jdk17.tar.gz \
  "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse"
tar -xzf /tmp/jdk17.tar.gz -C ~/.local/share/jdk17 --strip-components=1
~/.local/share/jdk17/bin/java -version   # Temurin-17.0.20.1+1
```

The system Java 26 is left untouched. Only these commands use JDK 17, via
`JAVA_HOME`.

### Android SDK

Installed from the official command-line tools zip (no sudo, no pacman) into
`~/Android/Sdk`. The zip expands to a `cmdline-tools/` directory which must end
up at `$ANDROID_HOME/cmdline-tools/latest`:

```sh
mkdir -p ~/Android/Sdk/cmdline-tools
curl -sL -o /tmp/cmdline-tools.zip \
  "https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip"
unzip -q /tmp/cmdline-tools.zip -d /tmp/cmdline-extract
mv /tmp/cmdline-extract/cmdline-tools ~/Android/Sdk/cmdline-tools/latest
```

Accept the licences, then install the packages:

```sh
yes | sdkmanager --licenses
sdkmanager "platform-tools" "emulator" \
           "platforms;android-35" "build-tools;35.0.0" \
           "system-images;android-35;google_apis;x86_64"
```

Gradle then auto-installs what the generated project actually asks for on the
first build, so you do not need to request these by hand:

| Package | Version |
| --- | --- |
| `platforms;android-36` | Android 36 (the project's `compileSdk`/`targetSdk`) |
| `build-tools;36.0.0` | 36.0.0 |
| `ndk` | 27.1.12297006 |
| `cmake` | 3.22.1 |

Use the `google_apis` system image, **not** `google_apis_playstore`: the
Play Store images are not rootable and are unnecessary here.

## Environment variables

Export these in every shell that touches the emulator or Gradle:

```sh
export JAVA_HOME="$HOME/.local/share/jdk17"
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export ANDROID_AVD_HOME="$HOME/.android/avd"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
```

`ANDROID_AVD_HOME` matters. On this machine `avdmanager` honours
`XDG_CONFIG_HOME` and writes AVDs to `~/.config/.android/avd`, but the
`emulator` binary only searches `$ANDROID_AVD_HOME`, `$ANDROID_SDK_HOME/avd`,
and `$HOME/.android/avd`. Without the export the emulator fails with
`Unknown AVD name [daily_mirror]`. The existing AVD lives at
`~/.android/avd/daily_mirror.avd`.

## The AVD

Created once, already present as `daily_mirror`:

```sh
echo "no" | avdmanager create avd \
  -n daily_mirror \
  -k "system-images;android-35;google_apis;x86_64" \
  -d pixel_7
```

Then edit `~/.android/avd/daily_mirror.avd/config.ini` so it contains:

```ini
hw.ramSize=2048
hw.keyboard=yes
```

Confirm with `emulator -list-avds` (not just `avdmanager list avd`, which reads
a different search path).

## Boot the emulator headlessly

```sh
emulator -avd daily_mirror -no-window -no-audio \
         -gpu swiftshader_indirect -no-boot-anim &
adb wait-for-device
until [ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = "1" ]; do
  sleep 2
done
adb devices
```

A cold boot takes about **28 seconds** on this machine. `adb devices` then
shows `emulator-5554  device` and `adb shell getprop ro.product.model` reports
`sdk_gphone64_x86_64`.

## Build and install the debug dev client

The native Android project is generated, not committed — the root
`.gitignore` already ignores `mobile/android/`. Regenerate it any time:

```sh
npm ci                     # only if node_modules/ is missing
cd mobile
npx expo prebuild --platform android
```

`mobile/app.config.ts` must declare `android.package`; it is set to
`app.dailymirror.android`. Without it `expo prebuild` refuses to run, because
the config is dynamic (TypeScript) and Expo cannot write to it.

Prebuild prints a harmless note that `userInterfaceStyle` needs
`expo-system-ui`; the app builds and runs without it.

Then build and install:

```sh
cd mobile/android
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

The first build takes about **9 minutes** (637 tasks, downloading Gradle
9.3.1, the NDK, and all dependencies) and produces a 211 MB debug APK — it
contains all four ABIs, per `reactNativeArchitectures` in
`mobile/android/gradle.properties`. The installed package is
`app.dailymirror.android`. Later builds are much faster.

`npx expo run:android` also works, but going through Gradle plus `adb install`
directly keeps the build and the install steps separately debuggable.

## Start Metro and open the app

```sh
adb reverse tcp:8081 tcp:8081
EXPO_NO_TELEMETRY=1 npm run mobile &        # from the repo root
# wait for: curl -s http://127.0.0.1:8081/status  ->  packager-status:running
adb shell am start -a android.intent.action.VIEW \
  -d "dailymirror://expo-development-client/?url=http%3A%2F%2Flocalhost%3A8081"
```

`adb reverse` is what lets the emulator reach the host's Metro on
`localhost:8081`. The deep link uses the app's `scheme` from
`mobile/app.config.ts` (`dailymirror`) and tells the dev client which bundler
to load, so you never have to type a URL in the emulator UI.

The first bundle takes a few seconds (`Android Bundled 6017ms mobile/index.ts
(1197 modules)`). The dev client shows its developer-menu sheet on first
launch; dismiss it with `adb shell input keyevent KEYCODE_BACK` before taking
screenshots. The sign-in screen then renders: Daily Mirror heading, username
and password fields, Sign in button, and the Server settings link.

Set `CI=1` alongside `EXPO_NO_TELEMETRY=1` for a non-interactive Metro that
does not watch for changes — useful for scripted runs, but leave it off when
you want fast refresh.

## Screenshots

```sh
adb exec-out screencap -p > app.png
```

`exec-out` (not `shell`) is required, otherwise the PNG is corrupted by
line-ending translation.

## Screen recording

`screenrecord` writes to the device, so record then pull:

```sh
adb shell screenrecord --time-limit 3 /sdcard/clip.mp4
adb pull /sdcard/clip.mp4 ./clip.mp4
```

The result is H.264 1080x2400. `screenrecord` caps a single clip at 180
seconds, so longer captures need several clips.

### Stitch clips with ffmpeg

Concatenating without re-encoding works because every clip has identical codec
parameters:

```sh
printf "file '%s'\n" "$PWD/clip1.mp4" "$PWD/clip2.mp4" > clips.txt
ffmpeg -y -f concat -safe 0 -i clips.txt -c copy stitched.mp4
ffprobe -v error -show_entries format=duration -of default=nw=1 stitched.mp4
```

Use absolute paths in `clips.txt`, or paths relative to the list file. ffmpeg
warns about a zero-duration data stream and auto-inserts the
`h264_mp4toannexb` filter; both messages are expected and harmless.

## Shut down

```sh
adb emu kill                      # stops the emulator
pkill -f 'expo start'             # stops Metro
```

`adb emu kill` returns before qemu actually exits, so poll until
`pgrep -af 'qemu[-]system-x86_64-headless'` produces no output — that is the
authoritative check. `adb devices` may briefly still list `emulator-5554` as
`offline`; clear that stale entry with `adb kill-server`.

## Helper script

`scripts/android-emulator.sh` exports the environment above and wraps each
step:

```sh
./scripts/android-emulator.sh boot                  # headless boot, waits for boot_completed
./scripts/android-emulator.sh install               # gradlew assembleDebug + adb install
./scripts/android-emulator.sh app                   # adb reverse, Metro, deep-link the dev client
./scripts/android-emulator.sh screenshot out.png
./scripts/android-emulator.sh record 10 out.mp4
./scripts/android-emulator.sh stop                  # emulator + Metro
```
