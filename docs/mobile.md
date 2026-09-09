# Daily Mirror for iOS

The Expo app in `mobile/` uses native React Native views, navigation, date
pickers, image decoding, and iOS scroll/zoom gestures. It has no WebView.

## Repository boundaries

| Directory | Responsibility | Deployment |
| --- | --- | --- |
| `mobile/` | Expo / React Native iOS client | Xcode, internal distribution, TestFlight |
| `server/` | Rust API and existing React website | Existing Vercel / Docker workflow |
| `device/` | Pi camera, button, LEDs, durable upload queue | Existing Pi deployment |
| `processor/` | Face-processing worker | Existing worker deployment |
| `packages/api/` | Portable fetch client and generated Rust API types | Bundled with clients |
| `crates/vision-contract/` | Server / worker Rust wire contract | Bundled with Rust apps |

The root npm workspace contains `mobile/` and `packages/*`. The NextRS server
retains its own npm workspace and lockfile: its generated `@server/client`
package, deployment scripts, and bundler depend on that layout. Native builds
need only the root workspace, not Rust, OpenCV, or the web dependencies.
Both TypeScript clients derive their contracts from the same Rust OpenAPI.

After changing API schemas, run `npm run api:generate` at the root and commit
`packages/api/src/schema.d.ts`. This runs the existing NextRS generator before
generating the portable declarations. Never edit the generated declarations.

## Run locally

Use Node 22.13+ and npm. The app currently targets Expo SDK 57 / React Native
0.86; use an SDK-compatible development build.

```sh
npm ci
npm test
npm run typecheck
npm run mobile:export  # Bundle the iOS JS/assets; does not compile/sign an IPA.
npm run mobile        # Metro for the development client.
```

On a Mac with full Xcode selected, an installed iOS simulator runtime, and
CocoaPods available:

```sh
npm ci
npm run ios
```

The sign-in screen defaults to `https://daily-mirror-pearl.vercel.app`. Use
**Server settings** to point at a local or preview server. The server must
include `/api/auth/login/native` for mobile sign-in. This endpoint was deployed
to the default production URL on September 7, 2026 from server-only PR #6.
Production builds require HTTPS. Debug builds permit HTTP for local work;
prefer HTTPS/Tailscale Serve for physical devices and respect iOS ATS rules.
Run the API through the existing `just dev` workflow. A phone cannot use
your workstation's `localhost`; enter its reachable origin.

No passwords, API secrets, or signing keys belong in Expo public config.

## Remote Mac (terakar; JSN fallback)

SSH must already be reachable, Remote Login enabled on the Mac, and the
host key trusted. Override the SSH target with `DAILY_MIRROR_MAC_HOST`, for
example `drew@terakar` (Tailscale IP `100.124.17.81`). The helper avoids the broken system SSH include
observed in the current Linux environment by using `-F /dev/null`; this also
means it does not read your usual SSH aliases or custom identity configuration.
It uses the SSH agent and default identities, with normal host-key checking.
The helper includes Homebrew Node 22 in PATH, sets a UTF-8 locale, and uses
`/Applications/Xcode.app` when the global selection is still Command Line Tools.

```sh
export DAILY_MIRROR_MAC_HOST=drew@terakar
./scripts/mobile-mac.sh doctor
./scripts/mobile-mac.sh sync
./scripts/mobile-mac.sh build
./scripts/mobile-mac.sh start
```

Sync copies only the mobile workspace and its shared packages into the Mac's
`~/work/daily-mirror-mobile`. It does not copy `.env` files, credentials,
photographs, server databases, or existing native build output. `build` uses
the iOS simulator, without starting Metro; `start` starts Metro separately.
An already booted simulator makes remote builds deterministic; otherwise Expo
may prompt for a simulator. A graphical Mac session is needed to use Simulator.

`eas.json` includes development simulator, physical-device development,
preview, and production profiles. Before distributing, select the real Apple
team, confirm the provisional bundle ID `app.dailymirror.ios`, link an EAS
project if using EAS, and configure signing. No Apple/EAS credentials or
project IDs are assumed or embedded here.

## Thumbnail and offline behavior

- The server already creates WebP thumbnails with a maximum edge of 320 px.
  The grid only loads `thumbnail_url`; missing previews show a processing
  placeholder and never fall back to full-size originals.
- Each server/account pair gets a separate SHA-256-named cache directory.
  Each revisioned image URL gets a SHA-256 filename. Tokens and expiring
  R2 URLs are never cache keys. A fresh session for the same account can reuse
  files across app launches; explicit sign-out clears them.
- Bearer-authenticated responses use `Cache-Control: private, no-store` so
  the native HTTP stack does not keep a second, URL-only disk cache outside
  these account-scoped controls. The shared image cache owns reuse.
- The cache has a **1 GiB** least-recently-used budget shared by every image
  surface, a **512 KiB** preview limit and **32 MiB** full-photo limit, and at most
  **three concurrent downloads**. Repeated requests share one in-flight download.
  Content length and streamed byte limits reject oversized responses;
  WebP/JPEG signatures are checked.
- A successful catalog refresh purges deleted photos and old revisions.
  Rotation changes the server's `?rev=` URL and therefore fetches fresh media.
  A network failure preserves the previous catalog. The cache lives in the
  OS cache directory; iOS can reclaim it under storage pressure.
- Gallery metadata persists for offline browsing within the existing session's
  30-day lifetime. An explicit 401 or local expiration signs out and clears
  private files. Offline browsing cannot detect remote revocation until the
  next successful connection.
- The viewer downloads full-size photos on demand into the same disk cache as
  archive thumbnails and flipbook face crops. Previously opened photos can be
  reused offline until evicted or cleared. Originals are not bulk-downloaded.
  Neighboring pages reuse saved originals or thumbnails while paging.
- Account settings show total image-cache usage and allow clearing it. Signing out
  cancels image requests, removes Keychain session data and private files,
  and clears image/query memory. Offline local sign-out explains that the
  unreachable server session expires on its normal schedule.

## Current functionality

Archive opens in Days. Pinch inward on the grid to move to Months, then Years;
spread outward to move back toward Days. Person and date filters can be combined.
Person filtering includes all confirmed/suggested photos, including multiple
photographs on the same day. The detail label supports VoiceOver
increment/decrement actions without restoring the top segmented tabs.

Flipbooks offer pull-to-refresh, a refresh button, and a reload when returning
to the tab. Tap the person's name to open the people picker. Playback follows
the newest day unless scrubbed backward; refresh preserves a selected day even
when earlier photographs are added. Confirmed and suggested face matches appear,
with one photograph per day and confirmed matches preferred on shared days.
Only manually confirmed matches contribute to recognition profiles; displaying
a suggested match does not train or confirm it.
The face-review button opens the website's `/admin`
page, which may require its own browser sign-in. Refreshing does not confirm
suggested matches.

Password sign-in, session restoration, native archive navigation, year/month/day
density, local-calendar date filtering, pull-to-refresh, offline thumbnail
browsing, full-screen paging, pinch/double-tap zoom, swipe-to-dismiss, rotation,
confirmed deletion, account information, passkey listing, and cache controls.

Native passkey sign-in is implemented using react-native-passkey and the native
start/finish API. It requires a rebuilt app signed by a team eligible for
Associated Domains; the free Personal Team is not eligible. The app and server
association must use the same signed App ID. See mobile-passkeys.md. Enrollment
continues through the existing web account, which may require a separate browser
sign-in. Admin/face-review tools remain on the web.

The photo viewer keeps swipe navigation and swipe-down dismissal. Its ellipsis
menu contains named rotation actions and deletion; deleting still requires a
separate confirmation.

## Verification before distributing

Implementation checks on September 7, 2026 passed TypeScript validation, all
11 mobile/shared-client tests, 21/21 Expo Doctor checks, iOS Hermes bundle
export, server Clippy and 42 server tests, and the existing device/processor
checks (one existing OpenCV-model test remains intentionally ignored).

On September 7, 2026, the native Debug build succeeded on terakar with
Xcode 26.5, with zero errors and two build-script warnings. The app installed
and launched on iPhone 17 Pro / iOS 26.5, Metro bundled 1171 modules, and the
native sign-in screen was visually verified. Both TypeScript checks and all
11 tests also passed on terakar. Signed-in gallery, cache, and gesture QA
remain outstanding and need an updated API plus a test account.

JSN's Xcode 16.4 build failed because ExpoModulesJSI needs Swift tools 6.2;
use Xcode 26 or newer for this app. See `docs/mobile-handoff.md` for the active
simulator, running Metro, logs, and the localhost IPv6 workaround.

Automated checks cover disk reuse across restarts, concurrency, size limits,
LRU eviction, missing files, revision/deletion invalidation, cancellation on
logout, date grouping/filtering, credential origin isolation, and the real
Rust native-login / bearer-auth / logout / shared-rate-limit HTTP flow.

On an iPhone or simulator, verify the following before a release:

1. Sign in against the updated server, force quit, reopen, and confirm the
   account restores. Reject invalid credentials and expired/revoked sessions.
2. Browse several hundred photos, revisit them, and restart the app. Confirm
   network inspection shows no repeat thumbnail GETs for cached revisions.
3. Enable airplane mode and revisit the archive. Cached thumbnails should
   remain; an uncached original should offer retry without losing its preview.
4. Pinch, double tap, pan a zoomed image, page between photos at 1x, dismiss
   vertically, rotate the phone, and check VoiceOver/large text/dark mode.
5. Rotate a test image, then confirm the grid and viewer show its new revision.
   Delete a disposable test photo and check it disappears on both clients.
6. Sign out while previews download, sign into another account, and confirm
   no previous account's thumbnails or catalog appear.

References: [Expo monorepos](https://docs.expo.dev/guides/monorepos/),
[Expo Image](https://docs.expo.dev/versions/latest/sdk/image/),
[Expo FileSystem](https://docs.expo.dev/versions/latest/sdk/filesystem/),
[iOS ScrollView zoom](https://reactnative.dev/docs/scrollview).
