# Vercel image processing verification

Verified September 7, 2026 (Mountain time), on production
[Daily Mirror](https://daily-mirror-pearl.vercel.app).
Final deployment: `dpl_J1gW7ZK55Cb8ZkAv5b3MBrgfj16c`,
[immutable URL](https://daily-mirror-1oyf0lama-ashirsc.vercel.app).
All three functions run in `pdx1`. Existing native password/passkey APIs and
Apple association were preserved from the concurrently deployed mobile work.

## What runs where

- `default`: pages, authentication, queue coordination, identity APIs and recovery cron.
- `images`: upload finalization, JPEG/WebP handling, thumbnails, rotation and crops.
- `vision`: existing YuNet detection, MediaPipe landmarks, SFace embeddings and
  database completion with conservative identity proposals.

An upload persists the job before starting a NextRS `WaitUntil` notification.
The worker claims one photo, renews its lease independently of native inference,
commits results, then notifies the next worker. A database lease serializes hosted
inference across instances. Five attempts is the limit; exhausted jobs remain
visible as failed. Crashed workers become eligible after lease expiry.
No separate queue service was added.

Recovery uses `#[nextrs::cron(schedule = "*/5 * * * *", provider = "cloudflare")]`
and `nextrs cron deploy`. The generated Cloudflare worker is only a timer;
processing remains on Vercel. The recovery HTTP request awaits a worker response.
The original daily reconciliation cron remains on Vercel. The shared cron secret
was synchronized on both providers; the local copy is in ignored `server/.env.local`.

## Measured function sizes

File lengths of final generated, uncompressed function trees:

| Function | Executable bytes | Native libraries | Models | Other | Total bytes | MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| default | 19,901,048 | 0 | 0 | 203 | 19,901,251 | 18.98 |
| images | 14,398,584 | 0 | 0 | 203 | 14,398,787 | 13.73 |
| vision | 13,951,240 | 65,598,512 | 42,687,538 | 203 | 122,237,493 | 116.57 |

Combined: **156,537,531 bytes**. An equivalent unsplit executable with both features
is **23,173,912 bytes**: the default executable is **3,272,864 bytes (14.12%) smaller**.
The unsplit baseline was compiled, not separately packaged. The vision function
has 127,762,507 bytes of headroom against 250,000,000 bytes before any provider
layers; provider-layer accounting is unavailable locally.

Vercel CLI separately reports **7.66 MB / 5.86 MB / 65.56 MB** for the deployed
functions. Those are provider-reported sizes, not the uncompressed file totals
above. Do not compare the two metrics directly. Exact inventories, hashes,
features and toolchain information are in [bundle-sizes.json](evidence/bundle-sizes.json).

## Verification

- 42 Rust unit tests, 2 native-session integration tests, 2 native-passkey
  integration tests, and 2 patched-runtime lifecycle regressions pass.
- Pinned NextRS client generation and TypeScript typecheck pass.
- 12 tests against the actual packaged Vercel adapters pass, including real native
  inference, isolated assets/routes, authorization, duplicate delivery and missed
  upload notification recovery. [Local evidence](evidence/local-packaged-verification.json).
- Real production face processing saved **478 landmarks**, a **128-value embedding**
  and an existing-library identity proposal (`centroid-v1`, score about 0.75).
  Warm request: **1,269 ms**, model load **0 ms**, peak process RSS **469,180 KiB**.
  Initial cold request: **3,005 ms**, model load **1,077 ms**, peak RSS **419,788 KiB**.
  These are individual observations, not load-test percentiles or a cost estimate.
- A production R2 signed upload finalized in **1.14 seconds** and completed
  automatically on its first attempt. No follow-up function request was made for
  30 seconds; the result was checked through the database. That worker chain also
  drained the eight originally pending photos. [Live evidence](evidence/live-inference-and-upload.json).
- The actual five-minute scheduled tick recovered a synthetic photo uploaded
  through the old deployment (which has no notification): pending/attempt 0 became
  complete/attempt 1 at `2026-09-08 04:46:04 UTC`. The generated timer logged HTTP
  **200**, with 3,483 ms wall time. No manual worker or cron request triggered it.
  [Scheduled recovery evidence](evidence/live-scheduled-recovery.json),
  [timer delivery log](evidence/cron-delivery.json).
- Final web login page hydrates with no observed console errors. Signed-in browser
  gallery navigation was not exercised because this browser has no active session;
  authentication and passkey behavior were covered by integration tests.

The three synthetic verification photos and their thumbnails/database records were
removed afterward. Final production queue: **59 complete, 0 pending, 0 leased,
0 failed**. All eight original pending images are processed.

## Operations and limitations

The compatible native build remains a prepared-cache workflow: see
[scripts/native/README.md](../scripts/native/README.md). Private models/libraries
are ignored build assets and must be preserved or restored from their upstream
sources. No assets are downloaded during a function invocation.

The temporary runtime patch is still required for reliable `WaitUntil` execution;
see [background-runtime-results.md](background-runtime-results.md). It fixes the
invocation lifetime, while the database and cron supply recovery. No sustained
load test or live forced-process-crash test was performed.

A different task deployed production during verification. Those newer native-auth
files were incorporated without changes, then the combined build was redeployed.
Original checkout modifications remain untouched; this implementation lives on
`codex/vercel-image-processing`. Future deployments must include these changes or
will remove hosted processing again.

Rollback: redeploy the previous native-auth production deployment
`dpl_CHg4tw5HYnd6jkDSKrw6WE9mxpdw` and resume the existing desktop processor.
Set `DAILY_MIRROR_HOSTED_PROCESSING=0` on a newly deployed build to disable hosted
processing. Existing leases expire; disabling does not cancel an invocation already
running. Keep the shared cron secret consistent when redeploying older source.
