# Native Vercel build

The workstation's OpenCV 5 libraries require newer glibc than Vercel supplies.
The checked-in Dockerfile builds OpenCV 4.12 against Debian 11. It is a **build
image**, not a deployed service. All application work runs in Vercel functions.

This is currently a prepared-cache workflow. The working cache on Drew's machine
is `/home/drew/.codex/visualizations/2026/09/07/01a07dcb-2e4c-74a3-8f49-4ded1aa2f42c`.
It contains `container-tools/bin/{nextrs,cargo-zigbuild,zig}`, `container-cargo`,
and `container-target`. The tools were compiled **inside Debian**, with Rust
1.96.0, NextRS commit `9fa4141d145f5afba869a42857e601c08a74e5fc` and
cargo-zigbuild 0.23.4. Do not substitute a host-compiled CLI requiring glibc 2.43.
`zig` points to the `/opt/zig/zig` mount. On this workstation set
`DAILY_MIRROR_ZIG_DIR=/home/drew/.local/share/ziglang-venv/lib/python3.14/site-packages/ziglang`.

Private assets are intentionally ignored, under `server/resources/vision`:
three checksummed models (hashes enforced by `server/src/vision.rs`), MediaPipe
0.10.35's `libmediapipe.so`, OpenCV shared objects, and their C++/JPEG/zlib/EGL/GLES
runtime dependencies. Keep only SONAME files, with no symlinks; use `$ORIGIN`
RUNPATH on each shared library. The complete deployed inventory and hashes are
in `docs/evidence/bundle-sizes.json`. There are no runtime downloads.

To recreate the Docker image, put the machine's CA bundle in a disposable build
context as `ca-certificates.crt` beside `Dockerfile`, then build it with the tag
`daily-mirror-native:bullseye`. The pinned Debian packages avoid the retired
Bullseye security archive's missing packages. Host package versions are untouched.
A fresh machine still needs the private model/library assets and Debian-compiled
tool cache prepared; `build.sh` does not bootstrap those prerequisites.

Generate the frontend and typed client with the pinned host NextRS CLI first,
then run `scripts/native/build.sh` with the two cache environment variables set.
It delegates bundle selection, compilation and packaging to `nextrs bundles build
--root server --vercel`. It uses G++/GNU libstdc++ consistently with OpenCV; Zig's
default libc++ otherwise produces incompatible C++ symbols.

Run `python scripts/test-hosted-processing.py`. Then deploy from `server/` using
`vercel deploy --prebuilt --target=production --yes --scope ashirsc`. This uploads
the generated artifacts without rebuilding against incompatible host libraries.
Do not run the old monolithic deploy script for this branch.

Set production hosted-worker variables listed in `server/.env.example`.
Deploy recovery with the pinned CLI's `nextrs cron deploy --root server`, passing
the current `CRON_SECRET` through its environment. Vercel cannot return sensitive
variable values after creation; preserve the shared value in a secret manager.
