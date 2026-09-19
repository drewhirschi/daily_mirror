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

### Hosted inference concurrency

`DAILY_MIRROR_HOSTED_CONCURRENCY` sets how many hosted inference workers may
hold a lease at once. It defaults to `1`, must parse as a whole number from `1`
to `8`, and an out-of-range or unparseable value is rejected when the worker
reads it rather than silently falling back, so a typo makes the invocation fail
visibly instead of quietly changing throughput. The variable belongs on the
vision bundle, which is where the claim gate runs; the web bundles only dispatch
to it. Each invocation still claims exactly one photo, so the variable is purely
a cap on how many invocations may be mid-inference for one pipeline version.
Raising it changes nothing else: per-photo dispatch from upload finalization,
the chained dispatch of the next photo, five-minute leases and the recovery cron
all behave as before, and a worker that finds the queue empty or already at the
limit returns its fast `idle-or-busy` report without claiming.

The ceiling is memory, not CPU. A single warm inference peaked at **469,180 KiB**
(about 470 MB) of process RSS on the 2 GB function measured above, and the
MediaPipe engine is cached per process, so concurrent invocations do not share
that cost. Four concurrent workers therefore sit near 1.9 GB of the 2 GB budget
with nothing left for the JPEG buffers and runtime overhead that ride along with
each request, which makes **4 the sensible ceiling on the default plan** and 2 or
3 the comfortable operating range; the allowed maximum of 8 is only reachable on
a function with a larger memory size. Exceeding the real budget does not degrade
gracefully — the function is OOM-killed mid-inference, which drops the lease
without a result, and the photo waits for its five-minute lease to expire before
the recovery cron retries it, so over-provisioning trades a small latency win for
much worse tail latency.

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

## PR preparation

Rebased onto `60d3872` after native authentication merged in PRs #6 and #7.
The PR preserves main's expanded session tests and removes duplicate native-auth
changes from its diff. Application behavior matches the verified deployment;
subsequent changes are formatting and test coverage. The artifact inventories
above describe the deployed build, before that history/formatting cleanup.
The recovery endpoint was invoked again while preparing the PR: HTTP 200,
59 complete and no pending, leased, or failed jobs.
