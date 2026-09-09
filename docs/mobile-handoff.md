## September 8 evening: JSN build and remaining installation blocker

Latest mobile source is synced to `/Users/drew/work/daily-mirror-mobile` on
`drew@jsn`. Xcode 26.6 is installed. Remote build commands explicitly set
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, since the system
selection still points at Command Line Tools. Node 22 and CocoaPods setup passed.
The prior native project is backed up as `native-before-september9.tgz`.

Implemented and checked:

- One account-scoped 1 GiB disk cache serves archive previews, flipbook crops,
  and full photos. Originals are saved on demand when opened. Logout, revisions,
  and explicit clearing retain their invalidation behavior. Different renditions
  remain separate cache entries; this does not prefetch the entire original library.
- Archive filters combine person and date. Person membership includes all assigned
  proposed/confirmed photos, rather than only a flipbook's one selected frame/day.
- Rotation and deletion are behind Photo options. Deletion retains confirmation.
- Days default, pinch density changes, flipbook refresh and person sheet are in
  the same build. Pinching currently regroups the archive from the top.

Mobile/shared typecheck, tests, and iOS Hermes export passed. JSN's physical
Release compilation passed with signing disabled. Its Release simulator build
passed with local ad-hoc signing, installed, and launched to a clean sign-in
screen. Authenticated interactions and physical-device installation remain
unverified. Logs and build products live in the remote project root, including
`jsn-unsigned-build.log`, `jsn-simulator-signed-build.log`, and
`build-jsn/Build/Products/Release-iphoneos/DailyMirror.app` (unsigned).

Physical signing still fails with `No Accounts` and no development profile for
`app.dailymirror.ios`. The user sees Andrew Hirschi's Personal Team in Xcode,
reselected it, and confirmed automatic signing is on. Both project configurations
use `C9P58ZP4AQ`. Retrying from SSH and from a temporary launchd job in the user's
desktop session produced the same error; that job was removed afterward. A valid
Apple Development certificate exists, but the only installed provisioning profile
is for a different app, TrustDeeds. Do not ask the user to sign in again without
new evidence. The exact Signing & Capabilities status is requested. Xcode's built-in
`xcrun mcpbridge` initialized, but its tool enumeration failed with BridgeError
Code 1 and a disconnected XPC connection; the bridge was stopped. No GUI build
was invoked through it. The configured
Associated Domains entitlement also requires an eligible team, as documented below.

JSN reaches iPhone `drew-25` at Tailscale `100.116.255.48`, but CoreDevice reports
its paired device unavailable. A TCP connection to 62078 opens, then the phone
resets the lockdownd QueryType request; wireless installation is not established.
An isolated diagnostic pymobiledevice3 environment exists at
`/Users/drew/work/daily-mirror-device-tools`. No new pairing or Tailscale Serve
configuration was created. No IPA has been signed or installed.

Server deployment preparation uses `/tmp/daily-mirror-flipbook-server`, branch
`codex/flipbook-suggestions-and-filters`, commit `0bae3ae`, based on production
`fe8c8c7`, preserving hosted processing and passkeys. All three native bundles,
12 packaged-server checks, and the full `just check` passed. Production deployment
`dpl_8ouuFjWshggQmyJ6TQobsAtwimU9` is Ready and aliased to
`https://daily-mirror-pearl.vercel.app`. Health and AASA return 200; unauthenticated
people and person-photo requests return 401. No authenticated production session
was used to verify the displayed flipbook. The deployment commit is also preserved
on this repository's local `codex/flipbook-suggestions-and-filters` branch.
Do not deploy the older monolithic server from this dirty working tree.

## September 8: include suggestions in flipbooks

The user clarified that unconfirmed matches should appear in flipbooks, while
only manually confirmed matches should teach recognition profiles. The local
server's people summaries and frames now include both `proposed` and `confirmed`
faces. Confirmed matches take priority when choosing a frame for the same day.
Unknown/rejected/dismissed faces remain excluded. The training query still
requires both `identity_state = 'confirmed'` and `identity_source = 'manual'`.
No production identities were edited. The mobile explanation and face-review
documentation now reflect this distinction. These server changes are now deployed (see the current entry above); the
installed app will pick them up when its people data refreshes.
All 40 server library tests, server library Clippy, mobile/shared TypeScript,
and mobile/shared tests pass. Regression coverage verifies suggested-day
inclusion, confirmed-frame priority, removal after rejection, and the existing
rule that suggestions never teach recognition profiles.

## September 8: processing diagnosis and navigation feedback

Production records were inspected on September 8 (Denver time), alongside the
Pi camera journal. All 60 ready photos have complete `face-v5` processing jobs;
there are no missing, pending, leased, or failed jobs in that pipeline.
Both September 8 captures have ready thumbnails and completed on their first
attempt, with no recorded error. The Drew match on
`20260908T143624Z-b41687e8` is `proposed`, not confirmed. Drew's latest confirmed
capture is September 6, which explains the flipbook's last day. No identities
were changed and no reprocessing was triggered. Older pipeline versions have
historical pending records; these are separate from the active v5 queue.

Local mobile changes:

- Archive defaults to Days, replaces the segmented tabs with pinch navigation
  through Days / Months / Years, and keeps VoiceOver adjustment actions.
- Flipbooks refresh by pull, toolbar button, and tab focus. Failed refreshes
  retain existing frames and offer retry. A name button opens a people sheet.
- The screen links to web face review. Its original confirmed-only explanation
  was replaced after the user's clarification above.
- Refresh follows the latest day or preserves the day selected by scrubbing.
  Photo edits and cache clearing also invalidate the people query.

TypeScript checks, the mobile/shared test suite, and iOS Hermes export pass.
No native dependencies were added. The Mac `terakar` and its Tailscale address
`100.124.17.81` both timed out over SSH; these changes have NOT been synced to
its Metro workspace or verified on the phone. When reachable, sync with
`DAILY_MIRROR_MAC_HOST=drew@terakar ./scripts/mobile-mac.sh sync`, preserving
its existing native project/signing setup. Validate pinch versus scroll and
pull-to-refresh, the picker sheet, and returning from browser face review on
the phone. The grid currently regroups from the top when its density changes.

# Expo iOS / JSN handoff

## Native passkey implementation (latest)

Native passkey login is now implemented. The server changes are isolated in
PR #7, branch `codex/native-passkeys`, commit `6f87c57`, based on merged PR #6.
Production is deployed at the usual daily-mirror-pearl.vercel.app domain.
The public AASA response and native passkey start endpoint were verified live.

Mobile uses react-native-passkey 3.6.1 with a guarded import for older installed
builds, native prompt cancellation handling, shared Keychain session storage,
and explicit RP/domain validation. Web enrollment remains available in Account.
Typecheck, mobile/API tests, and iOS production bundle export pass. Full server
pre-push checks pass, including signed software-authenticator login, replay
rejection, session revocation, association access, and rate limiting.

The real device's current profile is Personal Team C9P58ZP4AQ without Associated
Domains. Apple's capability table excludes free Personal Teams. An eligible
paid developer team is required to sign the configured entitlement. Update the
AASA App ID if the chosen team differs. An async membership question is pending.
TerraCar's native entitlement file now contains the webcredentials association;
this means Personal Team builds need an eligible team before they can sign.
Native Face ID testing remains blocked on that signing setup.

The simulator Debug link hit a prebuilt React Native configuration mismatch
(RCTPackagerConnection/Sealable symbols). The standalone Release simulator build
succeeded and launched without Metro. The sign-in screen visibly includes the
passkey button. The simulator displayed a saved-session restoration error, so
authenticated runtime behavior has not been verified in that unsigned build.
Logs: passkey-simulator-build.log and passkey-release-build.log
under /Users/drew/work/daily-mirror-mobile.

## Latest flipbook changes

Flipbooks now has its own native bottom tab, separate from Archive/Days.
Each person starts at their latest available frame (right end); drag left for
older days. Crop JPEGs share the existing account-scoped 48 MiB disk cache with
archive WebP thumbnails. Catalog reconciliation is scoped by media path so
photo and face refreshes do not purge each other's entries. The selected
person's frames warm sequentially newest first, leaving download capacity for
foreground requests. Playback now uses only the 384px face thumbnails, with no uncropped archive
preview or quality swap. Cached crops display directly; uncached crops show a
neutral loading state until ready. Full-resolution images are reserved for future
export work and are not requested by flipbook playback.
Tests cover cross-catalog retention, stale crop rejection, and disk reuse after
restart. Local typecheck/tests and TerraCar Metro bundle compilation pass.
No new native dependencies or server changes in this update.

## September 7: physical-phone feedback

The user has built and signed in successfully on drew-25 through TerraCar's
LAN Metro server at `http://192.168.4.55:8082`.

- Archive uses a filter icon and native Years/Months/Days segmented control.
- Bottom navigation uses React Navigation's native tab navigator, leaving the
  iOS system tab-bar appearance intact for Liquid Glass on supported iOS.
- Days includes person flipbooks from the existing `/api/admin/people` API,
  with a native slider and date-range filtering. Face crops use authenticated,
  same-origin requests and memory-only image caching.
- Thumbnail remount fades are removed, memoized thumbnails receive a stable
  selection callback, and the virtualized rendering buffer is enlarged.
  Verify inertial scrolling on the phone before declaring flicker resolved.
- Native passkeys remain unimplemented: web passkey completion issues a cookie;
  the app needs a native-session completion flow plus Apple's associated-domain
  entitlement and server AASA association. No server changes in this UI update.
- Added native packages require an iPhone rebuild, not just a Metro reload.
  Preserve TerraCar's `mobile/ios` directory and its existing signing fixes.
- Local typecheck, mobile/API tests, and production iOS bundle export pass.
- TerraCar compiled the updated native modules, but the SSH build failed signing
  `DailyMirror.debug.dylib` with `errSecInternalComponent`. Finish installation
  using Cmd-R in the already configured Xcode workspace on the Mac.
  Build log: `/Users/drew/work/daily-mirror-mobile/native-ui-build.log`.

## Objective

Continue the native Expo iOS app for Daily Mirror. The user wants the existing
web gallery/account functionality implemented with native components, reliable
local thumbnail caching, and a better photo viewer. Finish a real Xcode build
and test the native experience on the user's remote Mac.

Repository: `/home/drew/work/daily_mirror`.

## Current implementation

- `mobile/`: Expo SDK 57 / React Native 0.86 app with native archive and account
  tabs, password sign-in, Keychain session restoration, date grouping/filtering,
  pull-to-refresh, photo paging/zoom/dismissal, rotation, and confirmed deletion.
- `mobile/src/cache/`: persistent account/server-scoped thumbnail cache, 48 MiB
  LRU budget, 512 KiB per-image cap, three concurrent downloads, request
  deduplication, revision/deletion invalidation, and cancellation on logout.
  Gallery metadata is persisted for offline browsing. Originals load only in
  the viewer and are cached in memory.
- `packages/api/`: portable fetch client and committed TypeScript declarations
  generated from the Rust server OpenAPI. Run `npm run api:generate` after
  changing route schemas; never manually edit generated declarations.
- Rust server: new `/api/auth/login/native`, bearer authentication using the
  same revocable sessions/rate limits as web login, logout revocation, and
  OpenAPI declarations for mobile routes. Bearer responses disable HTTP
  caching so the app's account-scoped cache owns persistent thumbnail reuse.
- Root npm workspace contains mobile/shared packages. Existing server npm
  workspace and Rust device/processor layout were preserved.
- `scripts/mobile-mac.sh`: doctor, sync, simulator build, and Metro commands.
- `docs/mobile.md`: setup, architecture, caching behavior, and native QA list.
- CI/justfile include the new mobile checks.

The UI has NOT been run on iOS. Gesture behavior, layout, and cache persistence
need native validation; successful bundling alone does not establish that they
work correctly. Native passkey enrollment/sign-in is not implemented pending
Apple associated-domain setup. The app lists passkeys and links to web account
enrollment. Admin/face-review tools remain on the web.

## Verification already performed

- Mobile/shared TypeScript checks and 11 automated tests passed.
- Expo Doctor passed 21/21 checks.
- iOS Hermes JS/assets export passed (`npm run mobile:export`). This is not an IPA.
- Server Clippy, web TypeScript checks, and 42 server tests passed, including
  the native login/bearer/logout/rate-limit integration tests.
- Existing device and processor checks passed; one OpenCV-model test is
  intentionally ignored because it requires downloaded model assets.
- Shared schema regeneration produced identical declarations.
- Shell syntax and whitespace checks passed for accessible files.
- The aggregate `just check` initially needed `RUSTUP_TOOLCHAIN=1.96.0` in this
  environment and then stopped on a new Clippy warning. That warning was fixed
  and the remaining checks were run successfully individually.

Production server support has now been deployed; see the server-only PR and
verification above. The default production URL supports native sign-in.

## Physical build signing follow-up — September 7, 2026

The user selected their Personal Team in Xcode. Team `C9P58ZP4AQ` is now set
in the remote Xcode project and preserved in `mobile/app.config.ts` as
`ios.appleTeamId`. A valid Apple Development signing identity exists.
Expo's physical build compiled the app, but codesigning
`Debug-iphoneos/DailyMirror.app/DailyMirror.debug.dylib` failed with
`errSecInternalComponent` (Xcode exit 65). This likely requires allowing
codesign to access the login keychain from the Mac's graphical session.
Asked the user to select drew-25 in Xcode and press Cmd-R, approving any
codesign keychain prompt with their Mac login password. No password was
requested in chat or supplied by the agent. App installation is not confirmed.
Remote log: `~/work/daily-mirror-mobile/ios-device-build.log`.

After installation, launch using `xcrun devicectl device process launch
--device 2499B1AF-D115-503D-BE11-225CEAB04C40
--payload-url 'exp+daily-mirror://expo-development-client/?url=http%3A%2F%2F192.168.4.55%3A8082'
app.dailymirror.ios`. The phone's Metro on port 8082 is still running.

## Physical iPhone setup — September 7, 2026

`drew-25` (iPhone 17 Pro, iOS 26.6.1) is paired to terakar over USB and
Developer Mode is enabled. CoreDevice ID:
`2499B1AF-D115-503D-BE11-225CEAB04C40`; Expo resolves it as UDID
`00008150-001E28613C07801C`.

`npx expo run:ios --device drew-25 --no-bundler` finds the phone but stops:
`No code signing certificates are available to use.` The login keychain
reports zero valid code-signing identities. The user must add their Apple
account/development certificate and choose their team in Xcode. No physical
build has been installed yet.

A separate phone Metro is running on port 8082, advertised at
`http://192.168.4.55:8082` (terakar's LAN IP), with logs at
`~/work/daily-mirror-mobile/metro-device.log`. The simulator Metro on port 8081
is unchanged. Do not combine `--port` with Expo's `--no-bundler` flag.
Once signed and installed, open the device development client against port
8082. The phone must be able to reach terakar on the local network.

## Native QA follow-up — September 7, 2026

The user signed in on terakar and opened a real photo. A screenshot confirmed
that the viewer toolbar overlapped the Dynamic Island. Fixed PhotoViewer by
placing a SafeAreaProvider inside its full-screen Modal, reading insets in a
child component, and applying all four insets to controls and photo paging.
The date-filter sheet also now has its own provider. TypeScript passed and
the same open photo was visually verified after Fast Refresh: controls clear
the island and home indicator. Landscape and date-sheet interaction still
need visual QA. The sign-in password field also now has a show/hide eye button.

The user asked about testing on a physical iPhone. Terakar currently reports
no paired devices and zero valid code-signing identities. Asked whether they
prefer USB pairing to terakar or an EAS wireless install link; no choice has
been received yet. A local Xcode development build can use a personal Apple
account; EAS device distribution requires paid Apple Developer membership.
Do not reuse the currently running simulator Metro's advertised 127.0.0.1
URL on a physical phone: use a reachable Mac LAN/Tailscale address or a tunnel.
The existing `device` EAS profile is for physical development builds;
`development` is simulator-only. No physical app has been signed or installed.

## Production mobile API is live — September 7, 2026

Server-only PR: https://github.com/drewhirschi/daily_mirror/pull/6
Branch/worktree: `codex/native-session-api` at
`/home/drew/work/daily_mirror-server-mobile-api`.
Commit `c938869` contains only 10 server files (366 additions / 51 deletions,
including 228 lines of tests). Full local quality gate passed. The PR is open
for review; that commit has already been deployed through the existing
prebuilt script to `https://daily-mirror-pearl.vercel.app`.

Live smoke checks: native login now returns expected 401 JSON for deliberately
invalid credentials instead of 404, with `Cache-Control: no-store`; health is
OK, browser login is 200, and unauthenticated account API is 401. No database
migration or new configuration was needed. The user can retry native sign-in
against the default production URL. Real-account login and gallery QA remain
to be performed. Earlier missing-production-endpoint notes below are history.

Mobile, npm workspace, and hardware changes remain uncommitted in this original
checkout. They were excluded from the PR and production deployment.

## Current host: terakar — September 7, 2026

JSN is unavailable today. Use `drew@terakar` (spelling as registered in
Tailscale), IP `100.124.17.81`. SSH works. The Mac runs macOS 26.3.1(a),
Xcode 26.5 (17F42), Node 22.23.2, and CocoaPods 1.17.0.

- Synced into `~/work/daily-mirror-mobile`, preserving all local work.
- `npm ci`, both TypeScript checks, and all 11 tests passed on terakar.
- Native Debug build succeeded with zero errors and two build-script warnings.
- App `app.dailymirror.ios` installed and launched in iPhone 17 Pro / iOS 26.5,
  simulator UUID `F3E36FF5-1D4A-442B-B443-57C932C7D6A9`.
- Metro bundled 1171 modules and the native sign-in screen was visually verified.
- App and Metro are left running. Signed-in gallery/cache/gesture QA has not
  been performed; an updated API and a test account are still required.

The helper now adds Homebrew Node 22 to PATH, sets a UTF-8 locale, and uses
`/Applications/Xcode.app/Contents/Developer` when global xcode-select still
points at CommandLineTools. Verified the helper's `doctor` on terakar.

For a fresh SSH shell on terakar:

```sh
export PATH="/opt/homebrew/opt/node@22/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
export LANG=en_US.UTF-8
```

Metro was started in the mobile directory with
`REACT_NATIVE_PACKAGER_HOSTNAME=127.0.0.1 npx expo start --dev-client --lan`.
Do not use `--localhost` on this Mac: Node bound only IPv6 `::1`, while Expo's
bundle URL used IPv4 `127.0.0.1`, causing a connection error. The LAN-mode
listener resolves that mismatch. To reopen the project:

```sh
xcrun simctl openurl booted 'exp+daily-mirror://expo-development-client/?url=http%3A%2F%2F127.0.0.1%3A8081'
```

For a clean screenshot, the simulator app was launched with arguments
`-EXDevMenuIsOnboardingFinished YES -EXDevMenuShowsAtLaunch NO
-EXDevMenuShowFloatingActionButton NO`; these only suppress Expo's development
menu for that launch, without changing application source or persisted data.

Remote logs: `~/work/daily-mirror-mobile/ios-build.log` and `metro.log`.
Screenshot: `~/work/daily-mirror-mobile/simulator.png`; local copy:
`/tmp/daily-mirror-terakar/simulator.png`.

The JSN setup notes below are historical; terakar is the current build host.

## Latest JSN check — September 7, 2026

Xcode 16.4 is now installed at `/Applications/Xcode.app`; its first-run check
passes. The iOS 18.6 runtime is installed, and iPhone 16 Pro simulator
`60BB6982-A824-4B0B-A7CE-55682F908155` booted successfully. CocoaPods installation
completed (100 pods). The native build ran but failed in ExpoModulesJSI:
`package 'apple' is using Swift tools version 6.2.0 but the installed version
is 6.1.0`. Xcode 16.4 is insufficient for this Expo SDK 57 app despite meeting
React Native's declared minimum. Xcode 26 (Swift 6.2) requires macOS 15.6 or
later; JSN is still on 15.3.1. Update macOS and install/select Xcode 26 before
retrying. Do not lower the dependency's Swift tools version to mask this.
The failure logs are on JSN in `~/work/daily-mirror-mobile/mobile/.expo/`:
`xcodebuild.log` and `xcodebuild-error.log`. No app was installed or launched.

System `xcode-select` still points at CommandLineTools. For SSH commands use
`export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` along with
`PATH=/opt/homebrew/bin:/usr/local/bin:$PATH` and `LANG=en_US.UTF-8`.
Changing the global selection requires the user's sudo password; per-command
`DEVELOPER_DIR` works without it. Earlier missing-Xcode notes below are history.

## JSN verification — September 7, 2026

SSH now works from this session. `ssh -o BatchMode=yes drew@jsn hostname`
returns `jsn.localdomain`. The numeric IP also works with
`ssh -o HostKeyAlias=jsn -o BatchMode=yes drew@100.108.45.50 hostname`,
which verifies against the existing trusted `jsn` host key. The IP alone is
not in known_hosts; do not disable host-key verification. No profile repair
is needed in this session.

- JSN runs macOS 15.3.1 and has Node 22.22.0 / npm 10.9.4.
- Synced the mobile/shared workspace into `~/work/daily-mirror-mobile` with
  the existing helper, preserving local uncommitted work.
- `npm ci`, both workspace TypeScript checks, and all 11 tests passed on JSN.
- `CI=1 npx expo run:ios --no-bundler` successfully generated the native
  `mobile/ios` project on JSN. CocoaPods was absent; Expo began installing it
  via system Ruby. That attempt was stopped because full Xcode is missing.
- `xcodebuild -version` fails: the selected developer directory is
  `/Library/Developer/CommandLineTools`. No Xcode app was found in
  `/Applications` or by Spotlight. Only CommandLineTools exists under
  `/Library/Developer`. No native compilation or simulator QA has occurred.

## Next steps

1. Install full Xcode on JSN, complete its first-run setup, select its developer
   directory, and install an iOS simulator runtime. This requires Mac-side
   setup; Xcode is currently absent, not merely unselected.
2. CocoaPods 1.17.0 is now installed and verified. Homebrew could not build
   xcodes with the existing Command Line Tools; the official prebuilt xcodes
   2.0.3 binary is available and verified at
   `~/Downloads/daily-mirror-setup/xcodes` on JSN. Running `install 16.4`
   reached the Apple ID prompt and stopped because interactive credentials
   are required. Xcode 16.4 supports this Mac's macOS 15.3.1. Install it in an
   interactive SSH session, then select it, run Xcode first-run setup, and
   install the iOS 18.5 simulator runtime. Use `LANG=en_US.UTF-8` for CocoaPods.
3. Run `DAILY_MIRROR_MAC_HOST=drew@jsn ./scripts/mobile-mac.sh doctor`,
   then the helper's `build` command and `start`.
   The native project generated by Expo is already on JSN.
4. Test against an updated development/preview API; production still lacks
   the native login endpoint. Follow `docs/mobile.md`'s iOS QA checklist,
   especially gesture interactions and cache restart/logout behavior.
5. Confirm the Apple team/bundle identity before signing or distribution.
   The provisional bundle identifier remains `app.dailymirror.ios`.

## Preserve existing work

All implementation changes are uncommitted. Before this task, the user already
had modifications to `device/.env.example`, `processor/.env.example`,
`server/.env.example`, and `docs/hardware-prototyping-plan.md`, plus untracked
`docs/next-enclosure-plan.md` and `hardware/`. Do not overwrite or revert these.
The active filesystem rules denied reading `.env` and `.env.*` under work;
respect whichever restrictions are active in the new session too.

Read `server/AGENTS.md` before server edits. Do not hand-edit generated
`.nextrs/` output or generated process/deployment adapters.
