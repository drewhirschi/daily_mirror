# Submitting Daily Mirror to the App Store

Everything needed to get `app.dailymirror.ios` from this repository into App
Store review, split by who can do it. Written 19 September 2026 against Xcode
26.6 and Expo SDK 57.

Identity, as shipped:

| | |
| --- | --- |
| Display name | Daily Mirror |
| Bundle ID | `app.dailymirror.ios` |
| Team | `C9P58ZP4AQ` (Individual, paid) |
| Marketing version | `1.0.0` (`mobile/app.config.ts` → `version`) |
| Build number | `1` (`ios.buildNumber`) — **increase on every upload** |
| Deployment target | iOS 16.4 (Expo SDK 57 default) |
| Devices | iPhone only (`supportsTablet: false`, `UIDeviceFamily = [1]`) |
| Orientation | Portrait |
| Suggested category | Primary **Photo & Video**, secondary **Lifestyle** |

---

## Done in the repo

- **Identity and targeting.** Version `1.0.0` / build `1`, portrait only,
  iPhone only. iPad support was turned off deliberately: nothing in the app has
  been laid out for a tablet, and declaring it would oblige us to supply iPad
  screenshots and survive an iPad review pass.
- **Icon.** `mobile/assets/icon.png` is now 1024×1024 with no alpha channel, as
  the App Store icon requires. Expo generates every other size from it.
- **Export compliance.** `ITSAppUsesNonExemptEncryption = false` — the app
  speaks only standard HTTPS/TLS to its own server. This removes the export
  question from every upload.
- **Purpose strings.** Camera, Bluetooth (both keys), local network, location
  when in use, and Face ID all have specific, honest strings. The Face ID one
  matters: `react-native-passkey` ships the placeholder "Allow DailyMirror to
  access your Face ID biometric data", which was appearing in the built
  `Info.plist`; `app.config.ts` now overrides it.
  No photo-library permission is declared, because nothing reads or writes the
  library.
- **Privacy manifest.** `ios.privacyManifests` in `app.config.ts` declares
  `NSPrivacyTracking = false`, an empty tracking-domain list, and the four
  required-reason API categories the app actually touches (file timestamp
  C617.1, disk space E174.1, user defaults CA92.1, system boot time 35F9.1).
  Verified present in the built `DailyMirror.app/PrivacyInfo.xcprivacy`, along
  with the per-SDK manifests Expo bundles.
- **Account deletion (guideline 5.1.1(v)).** Implemented:
  `DELETE /api/account` (`server/app/api/account/route.rs`,
  `onboarding::delete_account`) and an Account → **Delete account** control in
  the app with a confirming alert. Rules:
  - The account, its password hash, passkeys, sessions and half-finished
    ceremonies go immediately.
  - If other accounts share the household, only the account leaves; the
    household and its archive carry on.
  - If the account is the **last one in its household**, the household is
    erased with it: every photograph and its stored R2 object, the faces and
    face embeddings derived from them, the people, the seating, and the
    cameras' claims.
  - A lone remaining **administrator with housemates** gets a 409 asking them
    to promote somebody first, rather than stranding everyone else.
  Covered by `server/tests/account_deletion.rs` (3 tests).
- **Placeholder and debug UI hidden in Release.** `mobile/src/store-build.ts`
  exports `STORE_BUILD = !__DEV__`, and it hides the "Invite to sign in" →
  "Invites are coming soon" dialog, the raw error line under a failed pairing
  step, and the "Server settings" field on the sign-in screen. All three stay
  in development builds, where they are the whole diagnosis.
- **Privacy and support pages.** `server/app/privacy/page.tsx` and
  `server/app/support/page.tsx`, linked from the site footer and reachable
  without signing in (`bypasses_authentication` in `server/src/view_auth.rs`).
  **Both are drafts and say so on the page.**
- **Repeatable archive.** `scripts/mobile-mac.sh store` prebuilds, archives
  Release and runs `-exportArchive` with
  `method = app-store-connect`, `destination = export` (never upload) and
  automatic signing, as a one-off GUI-domain LaunchAgent.

### Verified in the built archive

`DailyMirror.xcarchive` built cleanly on `drew@jsn`. Offline inspection of
`Products/Applications/DailyMirror.app`:

```
CFBundleIdentifier        app.dailymirror.ios
CFBundleDisplayName       Daily Mirror
CFBundleShortVersionString 1.0.0     CFBundleVersion 1
MinimumOSVersion          16.4       UIDeviceFamily [1]
UISupportedInterfaceOrientations  Portrait, PortraitUpsideDown
ITSAppUsesNonExemptEncryption     false
architecture              arm64 (no bitcode, as expected)
PrivacyInfo.xcprivacy     present, NSPrivacyTracking false
AppIcon60x60@2x.png       present
entitlements              application-identifier C9P58ZP4AQ.app.dailymirror.ios
                          com.apple.developer.associated-domains
                            webcredentials:daily-mirror-pearl.vercel.app
```

---

## Blocker: the archive cannot be exported for the App Store yet

`xcodebuild -exportArchive` fails on `jsn` with, verbatim:

```
error: exportArchive No signing certificate "iOS Distribution" found
error: exportArchive No Accounts
```

`security find-identity -v -p codesigning` on `jsn` lists exactly one identity,
`Apple Development: Andrew Hirschi (54KKL7Q67D)`. The archive is therefore
signed with the team provisioning profile (`get-task-allow = true`), which is a
development build and cannot go to App Store Connect.

**Drew, in the Xcode GUI on `jsn` (this cannot be scripted and needs your Apple
ID):**

1. Open Xcode → **Settings** → **Accounts**.
2. Select the Apple ID for team `C9P58ZP4AQ`. If it is not listed, add it.
3. Click **Manage Certificates…** → **+** → **Apple Distribution**.
4. Close, then re-run `DAILY_MIRROR_MAC_HOST=drew@jsn bash
   scripts/mobile-mac.sh store`. The `.ipa` lands in
   `~/work/daily-mirror-mobile/export/`.

The "No Accounts" line is the same cause seen from the export step: it reads
the Xcode account store, which is empty of a distribution identity.

---

## Drew must do (App Store Connect)

Nothing in this section can be done from the repo, and none of it was touched.

1. **Agreements.** App Store Connect → Business. A free app needs only the free
   (Paid Apps not required) agreement accepted. No tax or banking forms.
2. **Create the app record.** My Apps → **+** → New App.
   - Platform iOS, Name **Daily Mirror**, Primary language English (U.S.)
   - Bundle ID `app.dailymirror.ios` (appears once a build or App ID exists)
   - SKU: suggest `DAILYMIRROR-IOS-001`
   - Full access
3. **App Information.**
   - Category: Primary **Photo & Video**, Secondary **Lifestyle**
   - Privacy Policy URL: `https://daily-mirror-pearl.vercel.app/privacy`
   - Support URL: `https://daily-mirror-pearl.vercel.app/support`
   - **Both require a server deploy first** — the pages are in this branch but
     not live.
4. **Age rating questionnaire.** Suggested answers: no violence, no sexual
   content, no profanity, no gambling, no contests, no drugs, no horror, no
   unrestricted web access, no user-generated content shared publicly
   (photographs stay inside one household). Expected result **4+**.
5. **App Privacy.** Draft answers below.
6. **Screenshots.** Required sizes below.
7. **Metadata.** Drafts below.
8. **Review notes** with a demo account. Template below.
9. **TestFlight internal test first**, then submit for review.

### App Privacy answers (draft — read before submitting)

Declare data collection **Yes**. For every type: **linked to the user's
identity**, **not used for tracking**.

| Data type | Collected | Purpose | Notes |
| --- | --- | --- | --- |
| Photos or Videos | Yes | App Functionality | The archive itself |
| Sensitive Info (face data) | Yes | App Functionality | Face embeddings and landmarks used only to group photographs by person within one household |
| User ID | Yes | App Functionality | Account id, username |
| Name | Yes | App Functionality | Display name, household member names |
| Device ID | Yes | App Functionality | Paired camera identifiers |
| Product Interaction / Diagnostics / Usage Data | **No** | — | No analytics SDK of any kind |
| Contacts, Location, Health, Financial, Browsing, Search | **No** | — | Not collected |
| Advertising Data | **No** | — | No ad SDK |

Be conservative and declare face data under **Sensitive Info** rather than
arguing it is only "Other User Content". Guideline 5.1.2(vi) rules apply: face
data must not be used for marketing, advertising or any purpose other than the
feature the user enabled, must not be shared with third parties, and must be
deletable — the privacy policy says all three.

### Screenshots

Upload the two required iPhone sizes; App Store Connect scales the rest.

| Display | Device to capture on | Portrait pixels |
| --- | --- | --- |
| 6.9" | iPhone 16 Pro Max / 17 Pro Max | **1320 × 2868** |
| 6.7" | iPhone 15 Plus / 14 Pro Max | **1290 × 2796** |

(1242 × 2688 and 1284 × 2778 are also accepted for 6.7". Three to ten shots
each; the first three are what people see without scrolling.)

Suggested shot list, in order:

1. Gallery — a full day of photographs in the archive grid
2. A single photograph open, showing the capture details
3. A flipbook playing
4. Household screen — the people in the home, with enrollment status
5. Cameras screen — a paired camera with hardware and firmware
6. Account screen

### Metadata drafts

**Subtitle (30 chars max)** — `The year your home lived in`  (27)

**Promotional text (170)** —
`A quiet camera in your kitchen, a year of ordinary mornings you would never
have thought to photograph, and a flipbook of the people who live there.`

**Description** —

```
Daily Mirror is a small camera for your home and the archive that grows behind
it. It takes a photograph of the room the way it actually is — breakfast,
homework, the dog on the sofa — and files it away by the day and by the person
in it.

Nobody has to remember to take the picture. That is the whole idea. The
photographs you never think to take are the ones you want in ten years.

• An archive by day, month and year, so a Tuesday in March is one tap away
• Flipbooks that play a season back in a few seconds
• Everyone in the house recognised by name, so you can follow one person
  through the year
• Set up a camera from your phone over Bluetooth in about a minute
• Your household's photographs stay in your household. No advertising, no
  analytics, no third-party tracking of any kind
• Delete a photograph, or your whole account, whenever you like

A Daily Mirror camera fills the archive on its own. The app works without one:
browse your archive, play flipbooks, manage your household, and take
photographs with the phone.
```

**Keywords (100 chars, comma separated, no spaces)** —
`home,camera,family,photos,archive,memories,flipbook,timelapse,household,kids,album,daily`

### Review notes template

```
Daily Mirror needs a Daily Mirror camera to fill the archive on its own, so we
have prepared a demo account with a real household and a year of photographs.
Everything a reviewer needs is reachable without hardware.

Demo account
  Username: <FILL IN>
  Password: <FILL IN>

Note that account creation is disabled on this server, so please use the demo
account rather than signing up.

What works without the hardware
  • Gallery: the full archive, by day, month and year
  • Flipbooks
  • Household: the people in the home and their enrollment status
  • Enrollment capture: takes photographs with the iPhone camera
  • Account: including Delete account

What needs the hardware
  • Account > Cameras > Add a camera pairs a physical camera over Bluetooth.
    A video of the whole pairing flow on real hardware is attached.

Account deletion
  Account > Delete account, two taps. It deletes the login, password and
  passkeys immediately, and erases the household's photographs and face data
  when no one else is left in the household.

Face data
  The app recognises the people in the household to group their photographs.
  Face data never leaves the household's own storage, is never used for
  advertising or marketing, is never shared, and is deleted with the account.
```

Attach a screen recording of pairing on real hardware — guideline 2.1 requires
a demo video when a feature needs hardware the reviewer does not have.

---

## How to upload (after the distribution certificate exists)

Pick one. All three need Drew's credentials; none of them are in this repo.

**A. Xcode Organizer (simplest).** On `jsn`, open Xcode → Window → Organizer →
select the `DailyMirror` archive → **Distribute App** → App Store Connect →
Upload. Xcode signs and uploads with the account already in Settings.

**B. Transporter.app.** Drag `~/work/daily-mirror-mobile/export/DailyMirror.ipa`
into Transporter and sign in.

**C. Command line, with an App Store Connect API key.** Create the key at App
Store Connect → Users and Access → Integrations → App Store Connect API, and
put the `.p8` in `~/private_keys/` — **not in this repository**.

```bash
xcrun altool --validate-app -f ~/work/daily-mirror-mobile/export/DailyMirror.ipa \
  -t ios --apiKey <KEY_ID> --apiIssuer <ISSUER_ID>

xcrun altool --upload-app -f ~/work/daily-mirror-mobile/export/DailyMirror.ipa \
  -t ios --apiKey <KEY_ID> --apiIssuer <ISSUER_ID>
```

Then TestFlight → internal testing on Drew's own phone before submitting.

---

## Known review risks, worst first

1. **No demo account exists yet, and signup is off in production.**
   `DAILY_MIRROR_ALLOW_SIGNUP` is unset on prod, so a reviewer literally cannot
   create an account. A demo account with a populated household and a year of
   photographs is mandatory, not optional. Rejection is certain without it.
2. **Guideline 2.1 / 4.2: the app's core value needs hardware the reviewer does
   not have.** Mitigate with the demo account, the pairing video, and the fact
   that the archive, flipbooks, household and phone enrollment all work with no
   camera paired. Worth walking through the app once on a phone with no camera
   to be sure nothing looks broken.
3. **Face data under guideline 5.1.2(vi).** The app stores photographs of
   household members including children and computes face embeddings. The App
   Privacy answers and the privacy policy must agree exactly, and the policy
   must be live before submitting. Any mismatch is a rejection.
4. **Every authenticated session can read every household's photographs.**
   Noted in `server/app/api/auth/signup/route.rs`. Not an App Review blocker
   today because production has one household, but it contradicts the spirit of
   the privacy policy and should be fixed before anyone else has an account.
5. **The privacy and support pages are not deployed.** They exist on this
   branch only. The URLs must resolve before the listing can cite them.
6. **`app.dailymirror.ios` must be registered as an App ID with the Associated
   Domains capability.** Automatic signing has been doing this with
   `-allowProvisioningUpdates`, but confirm in the developer portal that the
   App ID exists with Associated Domains enabled, or the distribution profile
   will not include the entitlement and passkeys will break in the shipped app.
7. **Deleting a household's photographs is only reachable through account
   deletion.** `photos` has no `household_id`; the erase path derives the set
   from the household's cameras and its people's enrollment captures. That is
   correct for how the data is written today, but a photo whose camera was
   released would be missed. Worth a `household_id` column on `photos` before
   there is a second household.
8. **Default splash screen.** `expo-splash-screen` is not configured, so the
   launch screen is blank. Not a rejection, but it is the first thing anyone
   sees.

---

## Repeating the build

```bash
# from the repo root, with jsn reachable
DAILY_MIRROR_MAC_HOST=drew@jsn bash scripts/mobile-mac.sh sync
DAILY_MIRROR_MAC_HOST=drew@jsn bash scripts/mobile-mac.sh store
```

`store` runs `npm ci`, `expo prebuild -p ios --clean`, the Release archive and
`-exportArchive`. It writes `~/work/daily-mirror-mobile/store-build.log`, the
archive at `~/work/daily-mirror-mobile/DailyMirror.xcarchive` and the `.ipa` in
`~/work/daily-mirror-mobile/export/`. It never uploads anything.

The build takes long enough that the ssh connection waiting on it may time out
first. **That does not stop the build** — xcodebuild runs as a LaunchAgent on
`jsn` and carries on. It happened on both runs here. To see where it got to:

```bash
DAILY_MIRROR_MAC_HOST=drew@jsn bash scripts/mobile-mac.sh store-status
```

which prints whether the job is still running, the tail of the log, and whether
an `.ipa` was produced.

Before each upload, bump `ios.buildNumber` in `mobile/app.config.ts`.
