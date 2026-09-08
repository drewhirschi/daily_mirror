# Rust background lifecycle: live Vercel test

Verified 2026-09-07 on protected previews of `daily-mirror-bundle-trial`.
The diagnostic uses no photo storage, database, or application credentials.

| Runtime | HTTP response | Background ticks | Completion |
| --- | ---: | ---: | --- |
| Stock `vercel_runtime 2.4.0` | 0.303 seconds | 0 / 30 | Absent |
| Fin Streams lifecycle patch | 0.334 seconds | 30 / 30 | 30.034 seconds |

Each POST schedules thirty one-second waits using `nextrs::WaitUntil` and returns
immediately. After the POST, no further application requests were made to either
deployment. Logs were read through Vercel's control plane. Stock remained without
ticks after the full observation window. Patched logged every tick and completion.
This is one quiet-window run per runtime, not a concurrency/load or durability test.

- [Stock preview](https://daily-mirror-bundle-trial-gkreq0cy4-ashirsc.vercel.app)
- [Patched preview](https://daily-mirror-bundle-trial-qztm4v1n5-ashirsc.vercel.app)
- Preview deployment IDs: `dpl_7oDg1rEzrJn5yMfEj2Rbrg661BMv` (stock),
  `dpl_9HQdT6FKcnCCByHCnkveXVkgSt3b` (patched).
- Raw local probe events were lost during a workstation restart; the observations above were recorded before that restart.
- Probe source: `../tools/background-probe/src/main.rs`.

The pinned NextRS revision `9fa4141d145f5afba869a42857e601c08a74e5fc` already
provides `WaitUntil`. Its adapter forwards tasks to `AppState.wait_until`; the
problem is below that API. Stock Rust runtime reports the invocation's IPC `end`
before its registered work settles. The Fin Streams patch creates a per-request
collector and delays that signal while allowing the HTTP response to return.

Patch source: Fin Streams commit `0d3d23a543910852b78a3cc6fa209146403cf4c5`.
Its historical experiment is in commit `873911584aad761528c3855c7aeb8f01cae7418c`.
The patch is vendored under `../vendor/vercel_runtime` and selected by the server's
Cargo patch. The exact NextRS pin remains unchanged. Local regression tests prove
that one request cannot drain another's tasks, nested work is awaited, and a
background panic does not abort draining. Both passed.

The preview reports glibc 2.34, above the existing MediaPipe library's 2.27 minimum.
Actual native inference was subsequently verified in production; see
`vercel-image-processing-results.md` for package sizes, timing and result checks.

Keep durable work in the existing database queue. `WaitUntil` does not provide
retries, crash recovery or exactly-once execution. The eventual worker should
return success only after committing its result; use the lifecycle extension for
bounded dispatch work, with recovery for failed dispatches.

## Deployment notes

`vercel curl` successfully generated a project protection-bypass token using the
existing CLI login. Anonymous requests to the patched preview redirect to access
protection (HTTP 302); the probe POST through the CLI returned HTTP 200. No bypass
token is stored in this report.

Vercel automatically promoted the empty project's first diagnostic deployment
despite a default preview deploy command. That deployment was removed. Both URLs
above were then created with explicit `--target=preview`, and their runtime logs
confirm `environment=preview`. Daily Mirror production was untouched.

## Reproduce

Build `tools/background-probe/Cargo.toml` with `cargo zigbuild --release --target
x86_64-unknown-linux-gnu.2.26`. Set `PROBE_RUNTIME_LABEL=patched-2.4.0`. For the stock
control, use a disposable copy of the probe without its Cargo patch section and
with a separately resolved lockfile; label it `stock-2.4.0`.

Package each executable as a Rust executable function using the Vercel Build
Output API, with `supportsResponseStreaming=true` and `maxDuration=60`. Route
requests to that diagnostic function. This standalone probe is separate from the
framework-generated Daily Mirror deployment. Link only the isolated trial project
and deploy with `vercel deploy --prebuilt --target=preview --yes`.

POST `{"run_id":"unique-test-name"}` to `/probe` with `vercel curl --deployment
<url>`. Wait at least 40 seconds without application requests, then read
`vercel logs <url> --json --expand --since 10m`. Count all thirty ticks and the
completion event; record HTTP latency independently of task completion.
