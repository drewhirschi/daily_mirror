#!/usr/bin/env bash
# Build with the prepared Debian native/tool cache; see README.md beside this file.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/../.." && pwd)
build_cache=${DAILY_MIRROR_NATIVE_CACHE:?Set DAILY_MIRROR_NATIVE_CACHE to the prepared build cache}
zig_dir=${DAILY_MIRROR_ZIG_DIR:?Set DAILY_MIRROR_ZIG_DIR to the directory containing the Zig executable}
for path in "$build_cache/container-tools/bin/nextrs" "$project_dir/server/resources/vision/lib/libmediapipe.so"; do
  test -f "$path" || { echo "Missing prerequisite: $path" >&2; exit 1; }
done
docker run --rm --user "$(id -u):$(id -g)" \
  -e LIBCLANG_PATH=/usr/lib/llvm-11/lib -e PROJECT_DIR="$project_dir" \
  -v "$HOME/.rustup:/home/drew/.rustup:ro" \
  -v "$HOME/.cargo/bin:/home/drew/.cargo/bin:ro" \
  -v "$build_cache/container-cargo:/home/drew/.cargo" \
  -v "$HOME/.cargo/registry:/home/drew/.cargo/registry:ro" \
  -v "$HOME/.cargo/git:/home/drew/.cargo/git:ro" \
  -v "$zig_dir:/opt/zig:ro" -v "$build_cache:/build" \
  -v "$project_dir:$project_dir" daily-mirror-native:bullseye bash -c '
    set -euo pipefail
    export PATH=/build/container-tools/bin:/home/drew/.cargo/bin:/usr/bin:/bin
    export RUSTUP_HOME=/home/drew/.rustup CARGO_HOME=/home/drew/.cargo
    export CARGO_TARGET_DIR=/build/container-target XDG_CACHE_HOME=/build/cache
    export NEXTRS_SKIP_BUNDLE=1 PKG_CONFIG_ALLOW_CROSS=1
    export CXX_x86_64_unknown_linux_gnu=/usr/bin/g++ CXXSTDLIB_x86_64_unknown_linux_gnu=stdc++
    export RUSTFLAGS="-C link-arg=-Wl,-rpath,\$ORIGIN/resources/vision/lib -C link-arg=/usr/lib/x86_64-linux-gnu/libstdc++.so.6"
    cd "$PROJECT_DIR"
    nextrs bundles build --root server --vercel
  '
