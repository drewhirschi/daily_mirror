# Self-contained build image for the Vercel vision bundle.
#
# The workstation runs OpenCV 5 against glibc 2.44; Vercel's runtime is far
# older, so anything linked on the host is unusable there. This image pins a
# Debian 11 userland (glibc 2.31) and builds OpenCV 4.12 against it. It is a
# BUILD image, not a deployed service.
#
# Everything the build needs lives in the image: no host mounts for the Rust
# toolchain, no externally prepared tool cache. scripts/native/build.sh tags
# the image with a hash of this file, so editing it rebuilds once and every
# later build reuses the result.
FROM debian:bullseye-slim

# Bullseye's security archive is retired, so pin the packages that would
# otherwise resolve to withdrawn versions. Plain http avoids needing a CA
# bundle before ca-certificates is installed; apt verifies signatures anyway.
RUN printf 'deb http://deb.debian.org/debian bullseye main\n' > /etc/apt/sources.list \
 && apt-get -o Acquire::Check-Valid-Until=false update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --allow-downgrades --no-install-recommends \
      libc6=2.31-13+deb11u11 perl-base=5.32.1-4+deb11u3 \
      build-essential cmake pkg-config clang libclang-dev ca-certificates curl git \
      python3 python3-pip libjpeg-dev zlib1g-dev libegl1 libgles2 patchelf \
 && rm -rf /var/lib/apt/lists/*

# Only the modules the processor links; a full OpenCV would add hundreds of MB
# to every deployed function.
RUN curl -fsSL https://github.com/opencv/opencv/archive/refs/tags/4.12.0.tar.gz | tar xz -C /opt
RUN cmake -S /opt/opencv-4.12.0 -B /opt/ocv-build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/opt/opencv -DBUILD_LIST=core,imgproc,imgcodecs,dnn,objdetect -DBUILD_SHARED_LIBS=ON -DOPENCV_GENERATE_PKGCONFIG=ON -DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF -DBUILD_EXAMPLES=OFF -DBUILD_opencv_apps=OFF -DBUILD_JAVA=OFF -DBUILD_opencv_python2=OFF -DBUILD_opencv_python3=OFF -DWITH_IPP=OFF -DWITH_OPENCL=OFF -DWITH_TBB=OFF -DWITH_LAPACK=OFF -DWITH_FFMPEG=OFF -DWITH_GSTREAMER=OFF -DWITH_GTK=OFF -DWITH_QT=OFF -DWITH_V4L=OFF -DWITH_1394=OFF -DWITH_OPENGL=OFF -DWITH_JPEG=ON -DWITH_PNG=OFF -DWITH_TIFF=OFF -DWITH_WEBP=OFF -DWITH_OPENJPEG=OFF -DWITH_OPENEXR=OFF && cmake --build /opt/ocv-build -j 8 && cmake --install /opt/ocv-build && rm -rf /opt/ocv-build /opt/opencv-4.12.0
ENV PKG_CONFIG_PATH=/opt/opencv/lib/pkgconfig
ENV LD_LIBRARY_PATH=/opt/opencv/lib
# The opencv crate probes pkg-config for `opencv4`; the host override in
# processor/.cargo/config.toml names opencv5, so restate the container's truth.
ENV OPENCV_PKGCONFIG_NAME=opencv4

# Toolchain lives read-only in the image. CARGO_HOME is remounted writable at
# run time, so keep installed binaries outside it.
ENV RUSTUP_HOME=/opt/rust CARGO_HOME=/opt/cargo PATH=/opt/tools/bin:/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
      --default-toolchain 1.96.0

# Zig via the same distribution channel the workstation uses, so the pin is
# one version number in both places.
RUN pip3 install --no-cache-dir ziglang==0.16.0 \
 && ln -s "$(python3 -c 'import ziglang,os;print(os.path.join(os.path.dirname(ziglang.__file__),"zig"))')" \
      /usr/local/bin/zig

# nextrs must be compiled HERE: a host-built CLI needs glibc 2.43 and cannot
# run in this image. Keep the rev in step with server/Cargo.toml.
RUN mkdir -p /opt/tools \
 && CARGO_HOME=/opt/tools cargo install cargo-zigbuild --version 0.23.4 --locked \
 && CARGO_HOME=/opt/tools cargo install --locked \
      --git https://github.com/drewhirschi/nextrs.git \
      --rev 9fa4141d145f5afba869a42857e601c08a74e5fc cargo-nextrs \
 && rm -rf /opt/tools/registry /opt/tools/git

RUN chmod -R a+rX /opt/rust /opt/cargo /opt/tools /opt/opencv
