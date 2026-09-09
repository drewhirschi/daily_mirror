# Image processing on Vercel

Plan, 2026-09-07. Implementation and deployment have not started.

## Architecture

Reuse the existing processor, database job queue, and automatic identity
suggestions. Deploy three independently compiled functions:

| Function | Responsibilities |
| --- | --- |
| `default` | Pages, authentication, processing coordination, identity matching |
| `images` | Upload finalization, thumbnails, rotation, crops |
| `vision` | Existing YuNet/MediaPipe/SFace inference, native libraries and models |

Flow: **upload to private R2 → finalize → durable notification → vision worker
→ save landmarks, embeddings and identity suggestions**. Uploads trigger work
immediately; reconciliation remains a recovery mechanism.

## 1. Split the bundles

- Implement in a separate `codex/` branch/worktree, preserving uncommitted work.
  Pin both NextRS dependencies and the trial CLI to
  `9fa4141d145f5afba869a42857e601c08a74e5fc`; align the lockfile.
- Make codecs optional behind `image-processing`, and the processor dependency
  optional behind `face-inference`. Gate shared code and initialization too.
- Assign `/api/photos/**`, `/api/uploads/**`, `/api/admin/faces/[id]/crop`, and
  `/api/maintenance/reconcile` to `images`; give `vision` a narrowly authenticated
  run endpoint. Keep prefetch pages in `default` and all methods at a URL together.
- Generate supported NextRS configuration and clients before building. Preserve
  global auth, delegated upload/processor/cron checks, private cache headers and
  shared external stores. Do not hand-edit generated provider wiring.

## 2. Trigger background work reliably

- Atomically record the ready photo, processing job and an outbox notification;
  publish immediately after commit. Reconciliation republishes missed work.
- Send photo ID, pipeline version and job generation. Cover new uploads,
  rotation and retries; reject stale generations and fetch fresh R2 URLs on claim.
- Add claim-by-photo and a bounded single-image runner to the existing processor.
  Keep lease renewal independent of synchronous inference. Start at concurrency
  one; reserve time for model loading, download and completion.
- Acknowledge only committed completion, permanent failure or a confirmed no-op.
  Handle duplicates, expired leases, delayed retries and bounded attempts.
  Preserve conservative identity proposals and manual corrections.

## 3. Prove the two deployment risks early

**Queue integration:** Try Vercel Queues with a tiny synthetic-message preview.
Rust publishing has a REST API, but push-consumer support in this adapter is
unproven. The pinned NextRS split deploy rejects arbitrary provider settings,
so trigger generation needs a supported integration. A small SDK consumer calling
`vision` over authenticated HTTP is the fallback.
[Queue setup](https://vercel.com/docs/queues/quickstart).

**Native packaging:** Build CPU OpenCV, MediaPipe and their transitive libraries
for the actual Vercel Linux runtime and pinned build path. Bundle checksummed
models and libraries privately, with no runtime downloads. Target below 225 MB
against the ordinary 250 MB uncompressed function limit; verify actual runtime
memory/duration limits. If packaging fails, evaluate a separate Vercel container
service while retaining the split web functions.
[Function limits](https://vercel.com/docs/functions/limitations),
[container option](https://vercel.com/docs/functions/container-images).

## 4. Verify and report

- Run existing tests/typechecks; inspect route ownership and dependency graphs.
  Prove `default` excludes codecs/inference and only `vision` contains models.
- Test image outputs, landmarks, matching, authentication and private reads.
  Exercise duplicate delivery, publish failures, crashes, lease expiry, retries,
  and rotation/deletion during processing.
- Use a protected preview with isolated DB/R2/queue and synthetic images. Upload
  a photo and verify automatic completion without manually running cron or CLI;
  check authenticated browser navigation too.
- Measure cold/warm latency, peak memory, queue delay and cost per image.
  Distinguish local checks from live Vercel results.

**Required size report:** For `default`, `images`, `vision`, any delivery adapter,
and an equivalent unsplit baseline, report actual bytes and MiB for:

| Executable | Native libraries | Models / assets | Other files | Total uncompressed | Limit headroom |
| --- | --- | --- | --- | --- | --- |
| Measured after build | Measured | Measured | Measured | Measured | Measured or explicitly provisional |

Show the default-function reduction in bytes and percent, plus the combined size
of all functions. Retain build manifests and per-file inventories with exact
source/toolchain/features. Separate local, deployed and container measurements;
flag unavailable provider-layer sizes. Size alone does not prove faster startup.

Delivery order: **codec split → queue/native packaging proofs → background
integration → protected-preview verification and size report**.

No production deployment, migration, merge or crate publication in this trial.
Rollback: disable hosted dispatch and resume the existing desktop processor.
