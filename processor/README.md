# Daily Mirror processor

This Rust CLI is the intermittent compute client for face processing. It talks
to the deployed Daily Mirror server; it does not connect directly to R2 or
Turso.

Create one ignored configuration file for each server you want to process
against. For local development:

```sh
cp processor/.env.example processor/.env.local
```

The server and processor must share `DAILY_MIRROR_PROCESSOR_TOKEN` and
`DAILY_MIRROR_PROCESSING_PIPELINE` values.

From the repository root, read the local queue and drain it with:

```sh
just process-status
just process
```

Named environments map to `processor/.env.<name>`. For example, create
`processor/.env.production`, then use `just process-status production` and
`just process production`. These named files are ignored by Git. An invalid
environment name is rejected rather than being interpreted as a filesystem
path.

The default engine combines a full-frame MediaPipe pass with scaled OpenCV
YuNet detections used as additional region proposals. It runs the MediaPipe
Face Landmarker on each proposed crop, deduplicates overlapping results, and
generates aligned 128-dimensional SFace recognition embeddings. This hybrid is
important for distant or blurred faces: the full-frame landmarker can miss
them even though it works well once given the correct region.

The inference pipeline runs inside the Rust process. MediaPipe uses its XNNPACK
CPU delegate; local measurements found that faster and quieter than its Linux
OpenGL GPU delegate for this one-photo-at-a-time workload. The `--engine yu-net`
fallback retains the five-landmark OpenCV-only path. Model files live under
`processor/models/` and are intentionally ignored by Git. Their source URLs,
licenses, and checksums are recorded in `models/README.md`.

The direct CLI remains available from `processor/`. Without
`DAILY_MIRROR_ENV`, it retains the previous `.env` behavior:

```sh
cargo run --locked -- status
cargo run --locked --release -- process
```

Run its checks with:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
