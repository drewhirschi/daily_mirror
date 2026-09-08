# Temporary Rust runtime lifecycle patch

Base: crates.io vercel_runtime 2.4.0, Apache-2.0.
Source: vercel/vercel, packages/rust/runtime.
Patch copied from Fin Streams commit
0d3d23a543910852b78a3cc6fa209146403cf4c5 (2026-08-01), src/lib.rs only.
Upstream proposal: https://github.com/vercel/vercel/pull/17350
Report: https://github.com/vercel/vercel/issues/17351

Each request receives its own Awaiter. Its IPC end message is delayed until
registered wait_until futures settle; HTTP response delivery remains immediate.
Keep function duration limits and durable retries: this fixes invocation lifetime,
not crash recovery or exactly-once delivery. Remove when a verified upstream
runtime provides equivalent behavior. Collector comments were updated locally;
regression tests check request isolation, draining, and panic handling.

Daily Mirror verification is recorded in docs/background-runtime-results.md.
