# Temporary Xcode bridge over Tailscale

Verified September 8, 2026 with JSN (Xcode 26.6) and drew-25 (iOS 26.6.1).
JSN and the phone were on different LANs. The relay used the phone's Tailscale IP.

Results:

- Before: CoreDevice remembered the pairing but reported the device unavailable.
- Republishing the real RemotePairing advertisement through a local TCP relay
  changed the device to `available (paired)` and authenticated the control channel.
- Forwarding the negotiated TCP tunnel port enabled installed-app queries and
  successfully launched `app.dailymirror.ios` on the phone.
- Subsequently built and installed the password-only Release update over this
  bridge. Disabling Associated Domains allowed Personal Team provisioning;
  running the signing build in JSN's desktop session resolved SSH Keychain errors.

## Run

The bridge runs on **JSN**, not on the phone. Keep the phone unlocked on Wi-Fi
with Tailscale connected during initial discovery and installation. JSN must
already be paired with the phone. This workflow requires no Nuticast license.
The Linux computer is only needed for the initial Bonjour capture and SSH;
the app's traffic after installation goes to its configured backend.

Capture the phone's real service on a computer on its Wi-Fi while it is unlocked:

```sh
avahi-browse -rp _remotepairing._tcp
```

Use the resolved hostname, instance, port and all TXT values. Do not substitute
the CoreDevice UUID for the Bonjour instance. Advertisements can rotate.

Copy `scripts/coredevice-tailnet-bridge.py` to the paired Mac and run in a
foreground terminal (Python 3.9+; no third-party packages):

```sh
python3 coredevice-tailnet-bridge.py \
  --bind MAC_LAN_IP --phone PHONE_TAILSCALE_IP \
  --hostname PHONE_HOSTNAME.local --instance CAPTURED_INSTANCE \
  --service-port CAPTURED_PORT \
  --txt identifier=CAPTURED_IDENTIFIER --txt authTag=CAPTURED_AUTH_TAG \
  --txt ver=24 --txt minVer=8 --txt flags=0 \
  --ports OBSERVED_TUNNEL_PORT_RANGE
```

The TXT example must match the current capture. The initial test used service
port 49152. JSN's logs reported a TCP tunnel at 54395; forwarding 54390-54490
allowed the next connection at 54396. These are observations, not fixed Apple
port assignments. The script deliberately requires an explicit range and caps
it at 350 ports. It fails on a port conflict rather than taking over a listener.

Inspect the negotiated endpoint if device queries fail:

```sh
log show --last 2m --style compact \
  --predicate 'process == "remotepairingd"' | grep 'Got tunnel endpoint'
```

Check the connection using the full installed Xcode path when Command Line
Tools is the system default:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcrun devicectl list devices
```

The relay only accepts clients whose source is the Mac's bind IP or loopback;
it does not provide other LAN devices with access to the phone. Payloads are
forwarded without logging or terminating Apple's authenticated encryption.
Ctrl-C stops the relay and withdraws the advertisement. No launch agent, firewall
rule, hosts-file edit, or new pairing is installed.

## Build and install Daily Mirror

From the repository root on Linux, copy the latest source to JSN:

```sh
DAILY_MIRROR_MAC_HOST=drew@jsn ./scripts/mobile-mac.sh sync
scp -F /dev/null scripts/coredevice-tailnet-bridge.py drew@jsn:/tmp/
ssh -F /dev/null drew@jsn
```

On JSN, start the bridge above in its own terminal and keep it running. In a
second JSN terminal, use the existing native workspace:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:$PATH"
export LANG=en_US.UTF-8
cd /Users/drew/work/daily-mirror-mobile
npm ci
cd mobile/ios
xcodebuild -workspace DailyMirror.xcworkspace -scheme DailyMirror \
  -configuration Release -destination 'id=00008150-001E28613C07801C' \
  -derivedDataPath ../../build-jsn -allowProvisioningUpdates \
  DEVELOPMENT_TEAM=C9P58ZP4AQ build
```

This assumes the native workspace is already generated and its pods match the
source, as on JSN for this experiment. Native dependency/config changes require
updating that workspace and pods before building. A Release build embeds its
JavaScript, so Metro is not required after installation.

Only continue if the build succeeds **with signing enabled**:

If SSH reaches signing but fails with `errSecInternalComponent`, run the build
in a terminal on JSN's logged-in desktop, or as a one-off LaunchAgent in that
user's GUI domain. The verified installation used the latter; its log ended
with `DAILY_MIRROR_BUILD_EXIT=0`. Remove the one-off job after completion.
This is distinct from missing provisioning or unsupported entitlements and
does not call for signing out of the Apple account.

```sh
cd /Users/drew/work/daily-mirror-mobile
codesign --verify --deep --strict \
  build-jsn/Build/Products/Release-iphoneos/DailyMirror.app
test -f build-jsn/Build/Products/Release-iphoneos/DailyMirror.app/embedded.mobileprovision
xcrun devicectl device install app \
  --device 2499B1AF-D115-503D-BE11-225CEAB04C40 \
  build-jsn/Build/Products/Release-iphoneos/DailyMirror.app
xcrun devicectl device process launch \
  --device 2499B1AF-D115-503D-BE11-225CEAB04C40 app.dailymirror.ios
```

Do not uninstall the existing app first: installing the same bundle ID is the
normal update path and preserves its data when signing is compatible.

### Signing is separate from connectivity

An unsigned `.app` cannot be installed by this bridge. JSN needs a valid
development profile for `app.dailymirror.ios`, covering the iPhone and matching
its signing identity. `No Accounts` / `No profiles` from Xcode is a signing
failure even when `devicectl` can successfully launch the existing app.
Associated Domains supports native passkeys and requires an eligible developer
team. It is now commented out in `mobile/app.config.ts`, with native passkey
sign-in disabled in `mobile/src/auth-features.ts`, for the user-requested free
Personal Team build. Remove the corresponding key from an already generated
`mobile/ios/DailyMirror/DailyMirror.entitlements` too; source sync alone does
not update that file. Password login and Keychain session storage remain active.
The bridge does not grant additional signing capabilities.

This is an experimental foreground tool, not an unattended service. Re-capture
metadata and revise the observed port range after network or tunnel changes.
Long-running UDP sessions are not garbage-collected until shutdown. Signing
and provisioning remain separate requirements for installing a new build.

Based on the approach described in
[Kevin Paterson's implementation report](https://dev.to/kvnpt/how-to-remotely-iterate-deploy-your-sideloaded-ios-apps-over-tailnet-jak),
with local-client restrictions and ports chosen from JSN's actual logs. This
phone negotiated TCP, unlike the report's QUIC example.

## Lessons from the September 15, 2026 install

The archive-header/viewer update was built, installed, and launched this way.
Each item below cost one failed attempt:

- The phone's `_remotepairing` instance UUID and `authTag` rotate every few
  minutes and vanish when the phone locks. Capture with `avahi-browse` and
  start the bridge in the same command; a stale `authTag` produces
  `RemotePairingError 4` even though `devicectl` shows `available (paired)`.
- In zsh, `log` is a shell builtin. Use `/usr/bin/log show` or every log
  query silently returns nothing. The tunnel port shows up as
  `Connection refused` lines from `remotepairingd`; this time it was
  56046–56048, not the 54390–54490 range above.
- `--ports` counts the service port, so pass at most 349 ports
  (for example `56000-56348`). Each port opens a TCP and a UDP socket, so run
  `ulimit -n 4096` before starting the bridge or it dies with
  `Too many open files`.
- Build with `-destination generic/platform=iOS` so the build does not depend
  on the phone. SSH signing still fails with `errSecInternalComponent`; the
  one-off GUI LaunchAgent build works and should be removed with
  `launchctl bootout` afterward.
