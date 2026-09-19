# Mobile onboarding: household people and face enrollment

Status: implementation plan, 2026-09-15. This is the shared contract for the
server, API client, and mobile work. Change this document first when the
contract changes.

## Goal

A new user signs up from the phone, creates their household, and adds each
person in it. For every person, the app offers a guided capture: five photos
from five directions (lower left, upper left, center, upper right, lower
right). Each photo uploads as soon as it is taken. The server processes the
photos, attaches the single detected face to that person as confirmed
enrollment evidence, and reports per-photo status. When five photos are
enrolled, the person's recognition profile is active and later mirror photos
of them are matched automatically.

Manual tagging keeps working. A person who skipped capture, or whose profile
is weak, can be tagged from mirror photos through the existing face review
tools, and those confirmations sharpen the same profile.

## Decisions already made

- Onboarding photos are ordinary archive photos. They are tagged so they can
  be filtered or removed later, but nothing hides them today.
- Hosted inference may run several photos at once. The single-lease gate was
  a concurrency cap of one, not a correctness rule.
- Enrollment photos bypass the three-capture-day rule. Five confirmed
  enrollment faces enrol a person on their own. The manual path (five photos
  across three days) is unchanged, and both kinds of evidence feed one
  centroid.
- Mirror matches still land as `proposed`, not `confirmed`. Flipbooks and
  filters already show proposals, so the user sees auto-tagging immediately.
  Promoting matches to confirmed waits for threshold validation.
- Signup is gated by `DAILY_MIRROR_ALLOW_SIGNUP=1`. It stays off in
  production until per-household tenancy exists, because every session can
  currently read every photo and person.

## Data model changes

`users` (auth store, `server/src/auth.rs`):

| Column | Type | Meaning |
| --- | --- | --- |
| `household_id` | TEXT NULL | The household this user belongs to |
| `person_id` | TEXT NULL | The person record representing this user |

Both are added with the existing `ensure_column` pattern. The auth store and
catalog are separate stores, so these are plain references, not foreign keys.

`photos` (catalog, `server/src/catalog.rs`):

| Column | Type | Meaning |
| --- | --- | --- |
| `source` | TEXT NOT NULL DEFAULT 'device' | `device` or `enrollment` |
| `enrollment_person_id` | TEXT NULL | Person the enrollment photo is for |

`faces.identity_source` gains the value `enrollment`. It is set with
`identity_state = 'confirmed'` by processing completion when the photo is an
enrollment photo and exactly one face was detected.

No new tables.

## Domain logic

All onboarding logic lives in `server/src/onboarding.rs` as methods on the
existing stores, so HTTP routes and the CLI are thin adapters:

- `signup(username, display_name, password)`: creates the user, a household
  named after the user, a person with the user's display name, links them, and
  adds the person to the household. One failure rolls back what it can and
  returns an error; the CLI and route both surface it.
- `household_for_user(user)`: household config plus each member's enrollment
  summary.
- `add_household_person(user, display_name)`: creates the person and appends
  it to the user's household; fails when the household is full.
- `create_enrollment_upload(user, person_id, request)`: verifies the person is
  in the caller's household, reserves the photo with `source = 'enrollment'`
  and `enrollment_person_id`, and returns the same `UploadGrant` shape as the
  device flow.
- `finalize_enrollment_upload(user, person_id, photo_id)`: same checks, then
  `upload_flow::finalize_upload`, then `background::notify`.
- `enrollment_status(user, person_id)`: per-photo status and the enrolled
  flag.

Completion (`ProcessingQueue::complete` in `server/src/processing.rs`): after
inserting faces and before `propose`, if the photo has an
`enrollment_person_id` and exactly one face row was inserted, set that face
to `person_id = enrollment_person_id, identity_state = 'confirmed',
identity_source = 'enrollment'`. Zero or several faces leave the faces alone
and the status endpoint reports `retake`.

Matching (`server/src/face_matching.rs`): teaching faces are
`identity_state = 'confirmed' AND identity_source IN ('manual', 'enrollment')`.
Track an `enrollment_photos` count per profile. A profile is enrolled when
`enrollment_photos >= 5` or `(photos >= 5 AND days >= 3)`.

CLI: a new binary `daily-mirror-onboarding` in `server/src/bin/` with
subcommands `signup <username> [display-name]` (prompts for the password like
`daily-mirror-auth`), `household <username>` (prints the household summary as
JSON), and `add-person <username> <display-name>`. Add matching `just`
targets.

## HTTP API

All routes except signup are session authenticated (native bearer or web
cookie) and use the caller's linked household. A user without a household
gets 409 with a message telling them to sign up again or ask an admin.

| Method and path | Request | Response |
| --- | --- | --- |
| `POST /api/auth/signup` | `SignupRequest { username, display_name, password }` | `NativeSession` (same as native login), 403 when signup is disabled, 409 when the username exists |
| `GET /api/household` | | `HouseholdSummary` |
| `POST /api/household/people` | `CreatePersonRequest { display_name }` | `HouseholdPerson` |
| `POST /api/household/people/{person_id}/enrollment/uploads` | `UploadRequest { capture_id, content_type, content_length }` | `UploadGrant` |
| `POST /api/household/people/{person_id}/enrollment/uploads/{photo_id}` | | 204 |
| `GET /api/household/people/{person_id}/enrollment` | | `EnrollmentStatus` |

Types:

```text
HouseholdSummary {
  id, display_name, grid_size,
  self_person_id: string | null,
  people: HouseholdPerson[]
}
HouseholdPerson {
  id, display_name,
  enrollment: { enrolled: bool, enrolled_photos: number, required_photos: 5 }
}
EnrollmentStatus {
  person_id, enrolled: bool, enrolled_photos: number, required_photos: 5,
  photos: EnrollmentPhoto[]
}
EnrollmentPhoto {
  photo_id, captured_at,
  status: "uploading" | "processing" | "enrolled" | "retake" | "failed",
  face_count: number | null,
  thumbnail_url: string | null
}
```

`captured_at` follows the existing capture id convention. The `capture_id`
the phone sends must use the same `YYYYMMDDTHHMMSSZ-<8 hex>` format the Pi
uses so gallery date parsing keeps working.

Status mapping: photo row `pending` means `uploading`; ready with processing
`pending` or `leased` means `processing`; processing `complete` with one face
confirmed to this person means `enrolled`; complete with zero or several faces
means `retake`; processing `failed` means `failed`.

Signup also requires the same rate limiting as password login and the 12
character password rule already enforced by `AuthStore::create_user`.

## Hosted inference concurrency

`DAILY_MIRROR_HOSTED_CONCURRENCY` (default 1, must be 1 to 8) replaces the
`NOT EXISTS` single-lease gate in `claim_selected` with a count of unexpired
`vercel-%` leases below the limit. Everything else stays: one photo per
invocation, per-photo dispatch from upload finalization, chained dispatch of
the next photo, five-minute leases, recovery cron. Five parallel uploads
therefore fan out to five invocations.

Proof required: a unit test on `claim_selected` showing two hosted claims
succeed and the third is idle at limit 2, and a case in
`scripts/test-hosted-processing.py` that uploads several photos with the
limit above 1 and observes more than one `leased` row at the same time. Then
a preview deployment with the limit set to 4 processes five uploads and the
status endpoint shows overlapping leases.

## Mobile

New dependencies: `expo-camera` (with the config plugin and iOS/Android
permission strings). No face detection library; the server decides whether a
face was found. Guidance is a moving face outline animated with the built-in
`Animated` API to the five target positions; no 3D engine.

Screens:

- Sign in gains a "Create account" link to a Sign up screen (username,
  display name, password) that calls signup and enters the session.
- A Household screen reachable from Account and shown right after signup:
  lists members with an enrollment badge, "Add person" prompt, and a
  "Take photos" action per person.
- Guided capture: five slots, an animated outline moving lower left, upper
  left, center, upper right, lower right, shutter button, retake per slot.
  Each capture immediately requests an upload grant, PUTs the file, then
  finalizes. Uploads run in the background while the user continues. A slot
  shows uploading, processing, enrolled, or retake by polling the enrollment
  status every two seconds while any photo is unresolved.
- Done state: "Ready. New mirror photos of <name> will be tagged
  automatically." Then back to the Household screen.

Upload logic (grant, PUT, finalize, retry once on network failure, status
polling and slot state) lives in a plain `mobile/src/enrollment.ts` module
with `node:test` coverage using the fetch-stub pattern, since the test runner
cannot render components.

## Verification

- `just check` passes.
- Server unit tests for signup, household membership checks, enrollment
  auto-confirm with one face, retake with two faces, matcher enrollment by
  five enrollment photos, and concurrency claims.
- `npm test` covers the enrollment upload module.
- A preview deployment with signup and concurrency enabled, exercised end to
  end from the Android emulator, recorded as a video. See
  `docs/android-emulator.md`.

## Follow-ups not in this change

- Per-household tenancy for photos, people, and faces. Required before
  signup can be enabled in production.
- Promoting high-confidence mirror matches from proposed to confirmed.
- Excluding enrollment photos from flipbook day selection if they prove
  distracting.
- Removing a person and their enrollment photos from the app.
