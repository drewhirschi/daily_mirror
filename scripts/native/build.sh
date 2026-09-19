#!/usr/bin/env bash
# Compile the Vercel server bundles inside the Debian build image.
#
#   scripts/native/build.sh
#
# Writes server/.vercel/output. Deploy it with `vercel deploy --prebuilt`;
# scripts/deploy-prebuilt.sh does both. See README.md beside this file.
#
# The image is tagged with a hash of the Dockerfile, so it is built once and
# reused until the Dockerfile changes. Cargo's registry and target directory
# live in named Docker volumes, which is what keeps a warm build near 20s.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
project_dir=$(cd "$here/../.." && pwd)

tag=$(sha256sum "$here/Dockerfile" | cut -c1-12)
image="daily-mirror-native:$tag"

# The models and MediaPipe are private assets, deliberately gitignored, and
# nothing here can regenerate them.
lib="$project_dir/server/resources/vision/lib/libmediapipe.so"
test -f "$lib" || { echo "Missing private vision assets: $lib" >&2; exit 1; }

if ! docker image inspect "$image" >/dev/null 2>&1; then
  echo "==> building $image (first run compiles OpenCV; later runs reuse it)"
  docker build --tag "$image" "$here"
fi

# --user keeps generated files owned by the caller. HOME must be writable for
# cargo's scratch state, so point it at a volume rather than the image.
docker run --rm --user "$(id -u):$(id -g)" \
  -e HOME=/build/home \
  -e CARGO_HOME=/build/cargo \
  -e CARGO_TARGET_DIR=/build/target \
  -e XDG_CACHE_HOME=/build/cache \
  -e PROJECT_DIR="$project_dir" \
  -e NEXTRS_SKIP_BUNDLE=1 \
  -e PKG_CONFIG_ALLOW_CROSS=1 \
  -e CXX_x86_64_unknown_linux_gnu=/usr/bin/g++ \
  -e CXXSTDLIB_x86_64_unknown_linux_gnu=stdc++ \
  -e RUSTFLAGS='-C link-arg=-Wl,-rpath,$ORIGIN/resources/vision/lib -C link-arg=/usr/lib/x86_64-linux-gnu/libstdc++.so.6' \
  -v daily-mirror-build:/build \
  -v "$project_dir:$project_dir" \
  "$image" bash -c '
    set -euo pipefail
    mkdir -p /build/home /build/cargo /build/target /build/cache
    cd "$PROJECT_DIR"
    nextrs bundles build --root server --vercel
  '
