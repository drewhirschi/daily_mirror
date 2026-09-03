# Face-processing deployment exploration

Status: proposed exploration, 2026-09-02. This document does not authorize a
production deployment or a production-data processing run.

## Recommendation

Keep the existing NextRS web application on Vercel and keep face inference out
of its catch-all `api/index.rs` binary. The first deployment experiment should
be a **separate, CPU-only face service built from a container** and routed as
one narrowly authenticated endpoint. Vercel now supports both [Services][vercel-services]
and [OCI container images][vercel-containers] in beta, so this can remain one
Vercel project and one public origin without forcing OpenCV and MediaPipe into
every web request.

In parallel, make a smaller proof of concept for a dedicated ordinary Rust
Function. It may fit, but native-library compatibility and the 250 MB
uncompressed function limit make it the higher-risk packaging path. Do not
start a NextRS fork until that proof tells us which deployment boundary the
framework actually needs to generate.

If either Vercel route proves awkward or expensive, the current intermittent
desktop processor remains a complete, acceptable production mode. The queue
contract already makes processors replaceable.

## What exists today

Daily Mirror has two deliberately separate programs:

- `server/` is the NextRS/Axum web application. Vercel currently deploys it as
  one Rust binary, `api/index.rs`, behind a catch-all rewrite. All pages and API
  routes are compiled into the same Axum router.
- `processor/` is a Rust CLI. It claims leased work from the web API, downloads
  one image at a time, runs inference, and completes each image independently.
  A stopped worker does not mark an in-flight image complete, and expired
  leases become claimable again.

The `face-v5` inference process is CPU-only:

1. YuNet proposes faces.
2. MediaPipe Face Landmarker produces the 478-point mesh and refined bounds.
3. SFace produces the normalized 128-dimensional identity embedding.

The worker communicates through the existing authenticated processing API; it
does not need direct Turso or R2 credentials. That is a useful security
boundary and should remain the default even when the worker happens to run on
Vercel.

## Artifact and runtime reality

Measurements from the current local release build are:

| Artifact | Local size |
|---|---:|
| Rust processor executable | 7.4 MB |
| YuNet model | 0.23 MB |
| MediaPipe Face Landmarker task | 3.76 MB |
| SFace model | 38.70 MB |
| `libmediapipe.so` | 33.89 MB |
| Immediate OpenCV modules used by the local binary | about 68 MB |

That is already roughly 153 MB before OpenCV's transitive libraries. The local
OpenCV 5 package also pulls image codecs, BLAS/LAPACK, TBB, GL/EGL and X11;
recursively, the local shared-library closure is about 109 MB, though some of
that is supplied by the base operating system. This is a diagnostic from the
developer machine, not a deployable bundle-size measurement.

The normal Vercel Function limit is [250 MB uncompressed, including runtime
layers and bundled files][vercel-limits]. Vercel's large-functions beta does
not currently list Rust as a supported runtime. A generic function can use
[`includeFiles`][vercel-config], but the legacy `vercel-rust` documentation
warns that Rust's Lambda runtime links dynamically and native libraries must
be included and configured explicitly ([runtime source][vercel-rust-native]).

`libmediapipe.so` is not linked into the Rust executable; the Rust crate loads
it with `dlopen` on first use ([binding source][mediapipe-rs]). The copy currently tested locally requires at
least glibc 2.27 and has runtime dependencies on `libstdc++`, `libgcc_s`, EGL
and GLES even when inference uses the CPU delegate. The current local processor
binary is also dynamically linked to OpenCV 5. A binary built against the
developer machine's Arch libraries is therefore **not** a valid Vercel
artifact.

The practical packaging rules are:

- Build the executable, OpenCV, and all non-platform shared libraries in the
  same Linux environment that will execute them.
- Build a minimal, CPU-only OpenCV rather than copying the workstation's
  general-purpose package. Disable GUI, video, camera, OpenGL and unnecessary
  codecs; retain core, imgproc, imgcodecs/JPEG, dnn and objdetect plus whatever
  the SFace API actually requires.
- Vendor the three pinned models and the exact MediaPipe native library in the
  image/function. Disable MediaPipe's first-run download and set
  `MEDIAPIPE_LIB` to the deployed absolute path.
- Use an ELF `RUNPATH` such as `$ORIGIN/lib`, or set `LD_LIBRARY_PATH`, and test
  with `ldd`/`readelf` in the final runtime image.
- Use CPU only. Vercel Functions do not expose this computer's RTX GPU.

## Option A: one dedicated ordinary Rust Function

Vercel's official Rust runtime is currently beta and documents that each Rust
file under `api/` becomes a separate function ([Rust runtime][vercel-rust]). A
possible deployment surface is therefore:

```text
POST /api/internal/face-processing/run
    authenticate cron/processor secret
    initialize models once per warm function instance
    claim a small batch
    process and complete one image at a time
    stop before the invocation deadline
    return a compact summary
```

The request contains no JPEG. The function uses the existing queue API to get
an R2-backed download URL, avoiding Vercel's [4.5 MB request/response payload
limit][vercel-limits]. A cron invocation may call this endpoint, but cron is
only a trigger for a Function and inherits the Function's duration and resource
limits ([Cron Jobs][vercel-cron]).

### Why this is plausible

- The measured warm local inference is small enough for bounded batches.
- Vercel Rust supports Fluid compute and `waitUntil`, and normal Functions
  receive 2 GB/1 vCPU by default, with up to 4 GB/2 vCPU on eligible paid plans
  ([Rust runtime][vercel-rust], [function limits][vercel-limits]).
- Models can be initialized once in process-global state and reused by warm
  invocations.
- A separate `api/face_process.rs` entrypoint keeps the model and native
  libraries out of the NextRS web binary.

### Why it may fail

- The optimized uncompressed artifact may still exceed 250 MB.
- Vercel's current official Rust documentation does not promise arbitrary
  system packages. Native `.so` bundling must be proven, not assumed.
- Cold initialization could dominate a single-image run.
- CPU instructions used by prebuilt OpenCV/MediaPipe must be compatible with
  Vercel's fleet.
- Daily Mirror still pins the community `vercel-rust@4.0.11` adapter while
  Vercel now offers an official Rust runtime. Migration behavior, native-file
  tracing, Fluid support and NextRS streaming must be tested independently.
- Serverless invocations are bounded jobs, not a forever-draining worker.

### Required proof

Build a disposable, synthetic-photo function containing only:

- one health endpoint that reports model versions and library load success;
- one fixture bundled with the function;
- one inference call that reports cold initialization time, decode time,
  YuNet time, MediaPipe time, SFace time, peak RSS, and result dimensions.

Run `vercel build` and inspect `.vercel/output/functions/**/.vc-config.json`,
the complete uncompressed file tree, ELF dependencies and model presence before
any preview deploy. Then deploy a protected preview and test cold and warm
invocations. It must never connect to production data.

**Decision gate A:** continue only if the artifact is below 225 MB (leaving
headroom), every ELF dependency resolves, a cold invocation is reliable, and
one image finishes comfortably inside the configured duration. Otherwise move
to Option B without trying to hide missing libraries through runtime downloads.

## Option B: a Vercel container service

Vercel container images are beta on all plans. They run as Functions, accept a
normal HTTP server on `$PORT`, scale down after five idle production minutes,
and receive a 30-second SIGTERM grace period ([container images][vercel-containers]).
Vercel Services can build this container independently beside the NextRS web
service while sharing one deployment and route table ([Services][vercel-services]).

This is the best Vercel-shaped fit for the native stack:

```text
daily-mirror.vercel.app
  /api/internal/face-processing/*  -> face_worker service (Rust container)
  everything else                 -> web service (current NextRS app)
```

The container owns OpenCV, `libmediapipe`, the models, CA certificates and the
processor HTTP endpoint. It does not need to own the user-facing face APIs or
admin UI. The route should be secret-authenticated and preferably internal;
the web service can invoke it through a Vercel service binding, while a small
public cron trigger in the web service calls that binding.

Container images still follow Function duration, memory and billing limits.
They solve reproducible native packaging, not infinite execution. Each call
must claim a bounded number of images, complete them one at a time, and stop
with safety margin before `maxDuration`.

**Decision gate B:** choose this as the hosted design if the image builds
reproducibly, the service can reach the existing processing API, cold start is
acceptable for scheduled work, and measured cost is reasonable. Because both
Services and container images are beta, keep the desktop processor operational
as the rollback path.

## Option C: external CPU worker container

Package the same CPU-only worker image but run it on an ordinary container host
with a scheduled job or an always-on process. It continues using the public,
token-authenticated queue API, so no production database or object-store
credentials need to leave Vercel.

This is operationally less elegant than one Vercel project, but it has the
fewest runtime surprises: normal Debian libraries, predictable CPU, no
function bundle limit, and control over job lifetime. The worker can either
drain until empty or process a fixed maximum batch per scheduled run.

Choose this when Vercel's native packaging works but is too cold, too costly,
or too beta for the desired reliability.

## Option D: intermittent local-PC processor

This remains a first-class mode, not a failed deployment. The existing CLI is
already aligned with the desired interaction: start it, claim work, complete
each image independently, and stop with Control-C. The five-minute lease makes
abandoned work recoverable.

Improvements worth making even if hosted processing is deferred:

- add a `--max-images` and/or `--max-seconds` bound so local and hosted runners
  share exactly the same execution core;
- expose cold model-load and per-stage timing separately from the existing
  inference duration;
- add a dry-run fixture command that never claims production work;
- print the pipeline/model checksums at startup;
- keep the server-side reconciliation cron responsible only for ensuring work
  exists, not performing inference.

## What NextRS would need upstream

NextRS 0.6 currently emits one route registry, constructs one Axum router, and
the scaffold rewrites every dynamic path to `api/index.rs` ([deployment
documentation][nextrs-deploy]). Its own [roadmap][nextrs-roadmap]
already names “per-route Vercel binaries” as future work. There are two useful
upstream features, and they should not be conflated.

### 1. Multiple Rust Function groups

Add deployment groups to `nextrs.toml`, conceptually:

```toml
[[vercel.function]]
name = "face-processing"
routes = ["/api/internal/face-processing/**"]
entry = "api/face_processing.rs"
max_duration = 300
include_files = ["face-runtime/**"]

[[vercel.function]]
name = "web"
routes = ["/**"]
entry = "api/index.rs"
```

The exact schema needs design, but the framework should:

- generate a filtered registry per group while retaining one full registry for
  OpenAPI/client generation;
- generate one thin Vercel adapter per group;
- emit ordered rewrites so the specific face path wins before the web catch-all;
- preserve inherited middleware and shared application state for every moved
  route;
- validate that every route belongs to exactly one runtime group;
- support group-specific `maxDuration`, region, `includeFiles` and environment
  requirements;
- teach the prebuilt deploy copier to make **every** generated function
  self-contained, including declared data files and native libraries;
- add fixtures proving that a heavy dependency appears only in the face
  function artifact.

A crucial Cargo constraint is that dependencies are package-wide, not
bin-specific. Merely generating two bins in the existing `server` package may
still compile the native stack and can accidentally link it into both outputs.
The clean design is a separate workspace crate/package for the inference
function, with the API contract in the existing shared `vision-contract`
crate. NextRS can generate routing and adapters without pretending Cargo has
per-bin dependency sections.

### 2. Vercel Services/container generation

Services are now a more general deployment boundary than route binaries.
NextRS configuration generation should learn to preserve or generate Vercel's
top-level `services` map, move build/runtime configuration from the top level
into the NextRS web service, declare service bindings, and emit ordered
service-targeting rewrites. Vercel explicitly says that top-level `functions`,
`installCommand`, and `buildCommand` are invalid once Services mode is enabled,
so this cannot be implemented as a small extra JSON field
([Services configuration][vercel-services]).

Suggested upstream sequence:

1. Add a generic passthrough representation of service definitions and
   service-targeting rewrites to the NextRS config generator.
2. Add a fixture with the standard NextRS service plus a minimal Rust container
   sibling, and verify `vercel build` output without deployment.
3. Add service bindings and cron-route support.
4. Only then consider ergonomic route annotations or generated container
   scaffolds.

**Decision gate NextRS:** create a dedicated NextRS feature branch only after
the raw Vercel proof works. The branch should automate a verified deployment
shape; it should not be used to discover whether Vercel can load OpenCV or
MediaPipe.

## Phased execution plan

### Phase 0 — preserve the current boundary

- Keep `server` free of `opencv` and `mediapipe` dependencies.
- Record current web function size and cold/warm behavior as the regression
  baseline.
- Refactor no behavior yet; use synthetic fixtures only.

Exit: baseline measurements are saved and the production path is unchanged.

### Phase 1 — build a reproducible inference artifact

- Create a CPU-only Debian-based multi-stage build for the processor.
- Build the smallest required OpenCV modules from a pinned source release.
- Vendor checksummed YuNet, Face Landmarker, SFace and `libmediapipe.so`.
- Disable runtime downloads; set library paths explicitly.
- Run inference on fixed fixtures inside the final image and inventory the
  complete ELF closure and image layers.

Exit: the same container passes on the workstation and clean CI, with no host
libraries mounted.

### Phase 2 — test both Vercel packaging boundaries

- 2A: package the fixture as a separate official Rust Function and inspect the
  Build Output locally.
- 2B: package the fixture as a Vercel container service.
- Use protected previews only. No production secrets and no queue claims.
- Measure artifact/image size, cold start, warm inference, RSS, CPU time and
  failure behavior.

Exit: a written go/no-go result for each boundary.

### Phase 3 — connect a bounded worker to a non-production queue

- Extract one shared `run_bounded(max_images, deadline)` path used by CLI and
  HTTP entrypoints.
- Claim no more work than can finish before the deadline; initially use one
  image per invocation.
- Complete each image immediately. On SIGTERM/deadline, report retryable
  failure for the active lease where possible and leave unstarted work
  claimable.
- Require a processor/cron secret and reject browser session auth.
- Exercise duplicate invocation, lease expiry, retry, corrupt image and model
  initialization failure.

Exit: non-production jobs survive retries without duplicate completed rows or
lost leases.

### Phase 4 — automate the proven deployment shape in NextRS

- Implement either grouped Rust functions or Services support in an isolated
  NextRS branch, based on Phase 2's winner.
- Add config-generation, route-precedence, artifact-isolation and prebuilt
  deployment tests.
- Dogfood the branch in a Daily Mirror preview before proposing upstream work.

Exit: the NextRS-generated Build Output matches the hand-built proof and the
ordinary web function has not gained native inference dependencies.

### Phase 5 — optional production trial

- Requires explicit approval.
- Deploy the face service disabled by default.
- Enable a low-frequency cron or manual admin trigger with a one-image limit.
- Compare results and timing to the desktop worker for the same fixture set.
- Increase the bound gradually while watching cost, cold starts, lease errors
  and face-result parity.

Rollback is to disable the trigger. Pending work remains available to the
desktop processor.

## Decision table

| Question | Ordinary Rust Function | Vercel container service | External container | Local PC |
|---|---|---|---|---|
| Keeps NextRS web function small | Yes, if artifact isolation works | Yes | Yes | Yes |
| Native library control | Fragile; must bundle `.so` files | Strong | Strong | Already works |
| Vercel-only deployment | Yes | Yes | No | No |
| Continuous drain | No; bounded invocation | No; bounded invocation | Yes | While CLI runs |
| GPU | No | No | Host-dependent | RTX available, currently unnecessary |
| Platform maturity | Rust beta | Services/container beta | Provider-dependent | Stable locally |
| Recommended role | Packaging experiment | Preferred hosted experiment | Hosted fallback | Always-retained fallback |

## Final decision rule

Prefer the Vercel container service if its preview is reliable and affordable.
Use an ordinary Rust Function only if the optimized artifact has comfortable
size headroom and native loading is repeatable. Use an external container if
Vercel beta features or invocation limits are the problem. Otherwise continue
running the existing desktop processor occasionally; no queue or UI redesign
is required for that outcome.

[vercel-rust]: https://vercel.com/docs/functions/runtimes/rust
[vercel-limits]: https://vercel.com/docs/functions/limitations
[vercel-config]: https://vercel.com/docs/project-configuration/vercel-json#functions
[vercel-cron]: https://vercel.com/docs/cron-jobs
[vercel-containers]: https://vercel.com/docs/functions/container-images
[vercel-services]: https://vercel.com/docs/services
[vercel-rust-native]: https://github.com/vercel-community/rust#muslstatic-linking
[mediapipe-rs]: https://github.com/nikicat/mediapipe-rs#nothing-to-install
[nextrs-deploy]: https://nextrs-docs.vercel.app/docs/deploy-vercel
[nextrs-roadmap]: https://github.com/drewhirschi/nextrs/blob/main/ROADMAP.md
