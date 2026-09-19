set shell := ["bash", "-euo", "pipefail", "-c"]

pi_host := env_var_or_default("DAILY_MIRROR_PI_HOST", "drew@rpi1.local")
pi_admin := env_var_or_default("DAILY_MIRROR_PI_ADMIN_URL", "http://rpi1.local:8081")
device_target := "aarch64-unknown-linux-gnu.2.36"

# Show the available project commands.
default:
    @just --list

# Check the local tools used for development and deployment.
doctor:
    @command -v cargo >/dev/null || { echo "missing: cargo" >&2; exit 1; }
    @command -v nextrs >/dev/null || { echo "missing: cargo-nextrs" >&2; exit 1; }
    @command -v cargo-zigbuild >/dev/null || { echo "missing: cargo-zigbuild" >&2; exit 1; }
    @command -v zig >/dev/null || { echo "missing: zig" >&2; exit 1; }
    @command -v node >/dev/null || { echo "missing: node" >&2; exit 1; }
    @command -v npm >/dev/null || { echo "missing: npm" >&2; exit 1; }
    @command -v curl >/dev/null || { echo "missing: curl" >&2; exit 1; }
    @command -v ssh >/dev/null || { echo "missing: ssh" >&2; exit 1; }
    @command -v scp >/dev/null || { echo "missing: scp" >&2; exit 1; }
    test -f processor/Cargo.toml
    @echo "Daily Mirror development tools are ready"

# Install the locked web dependencies, including the project-local Vercel CLI.
install:
    cd server && node .nextrs/ensure-client.mjs && npm ci
    npm ci

# Configure this clone to run the full quality suite before every Git push.
install-hooks:
    git config core.hooksPath .githooks
    @echo "Daily Mirror pre-push quality gate installed"

# Run the local NextRS gallery and API.
dev:
    cd server && cargo dev

# Create a gallery account, prompting securely for its password.
auth-create-user username:
    cd server && cargo run --locked --bin daily-mirror-auth -- create-user "{{username}}"

# Create an account with its own household and person, prompting for the password.
onboarding-signup username:
    cd server && cargo run --locked --bin daily-mirror-onboarding -- signup "{{username}}"

# Print a user's household and per-person enrollment progress as JSON.
onboarding-household username:
    cd server && cargo run --locked --bin daily-mirror-onboarding -- household "{{username}}"

# Add a person to a user's household.
onboarding-add-person username name:
    cd server && cargo run --locked --bin daily-mirror-onboarding -- add-person "{{username}}" "{{name}}"

# Regenerate the typed web client after changing a Rust API route.
client:
    cd server && npm run client:generate
    npm run generate --workspace @daily-mirror/api

# Start Metro for the native Expo development client.
mobile:
    npm run mobile

# Build the iOS development client on this Mac.
mobile-ios:
    npm run ios

# Validate the shared client, native app, and iOS bundle without a Mac.
mobile-check:
    npm run typecheck
    npm test
    npm run mobile:export

# Format, compile, type-check, and test both Rust applications.
check:
    cd crates/mirror-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd firmware/host && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd device && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd processor && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd server && npm run client:generate && cargo fmt --check && cargo clippy --all-targets -- -D warnings -A clippy::match-single-binding && npm run typecheck && cargo test
    npm run generate --workspace @daily-mirror/api
    git diff --exit-code -- packages/api/src/schema.d.ts
    just mobile-check
    git diff --check

# Show one environment's pending, leased, complete, and failed processing counts.
# Examples: `just process-status` or `just process-status production`.
process-status environment="local":
    @test -f "processor/.env.{{environment}}" || { echo "missing processor/.env.{{environment}}; copy processor/.env.example and configure it" >&2; exit 1; }
    cd processor && DAILY_MIRROR_ENV="{{environment}}" cargo run --locked -- status

# Drain one environment's face-processing queue with the optimized binary.
# Examples: `just process` or `just process production`.
process environment="local":
    @test -f "processor/.env.{{environment}}" || { echo "missing processor/.env.{{environment}}; copy processor/.env.example and configure it" >&2; exit 1; }
    cd processor && DAILY_MIRROR_ENV="{{environment}}" cargo run --locked --release -- process

# --- ESP32 camera firmware -------------------------------------------------
# Every recipe below needs the ESP-IDF environment, which is a shell function
# rather than a binary, so each one sources export.sh itself.
#
# MIRROR_BOARD is cached in the build directory the first time it is
# configured, so only the build recipes pass it. Set DAILY_MIRROR_FW_EXTRA to a
# credentials sdkconfig OUTSIDE this repository to bake in bench defaults; see
# firmware/esp-idf/README.md.

idf_export := "source ${IDF_PATH:-$HOME/esp/esp-idf}/export.sh >/dev/null"
fw_extra := env_var_or_default("DAILY_MIRROR_FW_EXTRA", "")

# Build the ESP32-P4 + IMX519 firmware.
fw-build-p4:
    cd firmware/esp-idf && {{idf_export}} && idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 \
        {{ if fw_extra == "" { "" } else { "-DMIRROR_EXTRA_SDKCONFIG=" + fw_extra } }} build

# Build the ESP32-S3 + OV5640 firmware.
fw-build-s3:
    cd firmware/esp-idf && {{idf_export}} && idf.py -B build_s3 -DMIRROR_BOARD=s3_ov5640 \
        {{ if fw_extra == "" { "" } else { "-DMIRROR_EXTRA_SDKCONFIG=" + fw_extra } }} build

# Build both boards from whatever state the build directories are in.
fw-build: fw-build-p4 fw-build-s3

# Flash the ESP32-P4 over its CH343 bridge, failing loudly if a write is not verified.
fw-flash-p4 port="/dev/ttyACM0": fw-build-p4
    cd firmware/esp-idf/build_p4 && {{idf_export}} && \
        python -m esptool --chip esp32p4 -p {{port}} -b 921600 \
            --before default_reset --after hard_reset write_flash "@flash_args" \
        | tee /dev/stderr | grep -q "Hash of data verified" \
        || { echo "flash NOT verified - the board is still running the old firmware" >&2; exit 1; }

# Flash the ESP32-S3 over its CH340 bridge, failing loudly if a write is not verified.
fw-flash-s3 port="/dev/ttyUSB0": fw-build-s3
    cd firmware/esp-idf/build_s3 && {{idf_export}} && \
        python -m esptool --chip esp32s3 -p {{port}} -b 921600 \
            --before default_reset --after hard_reset write_flash "@flash_args" \
        | tee /dev/stderr | grep -q "Hash of data verified" \
        || { echo "flash NOT verified - the board is still running the old firmware" >&2; exit 1; }

# Watch the ESP32-P4 console (2 Mbaud; Ctrl-] to quit).
fw-monitor-p4 port="/dev/ttyACM0":
    cd firmware/esp-idf && {{idf_export}} && idf.py -B build_p4 -p {{port}} monitor

# Watch the ESP32-S3 console (921600 baud; Ctrl-] to quit).
fw-monitor-s3 port="/dev/ttyUSB0":
    cd firmware/esp-idf && {{idf_export}} && idf.py -B build_s3 -p {{port}} monitor

# Open menuconfig for one board. Example: just fw-menuconfig p4_imx519
fw-menuconfig board:
    cd firmware/esp-idf && {{idf_export}} && idf.py -B "build_{{ if board == "p4_imx519" { "p4" } else { "s3" } }}" \
        -DMIRROR_BOARD={{board}} menuconfig

# Throw away both firmware build directories.
fw-clean:
    rm -rf firmware/esp-idf/build_p4 firmware/esp-idf/build_s3 firmware/dependencies.lock

# Cross-compile the Pi service on this computer.
device-build:
    cd device && cargo zigbuild --locked --release --target {{device_target}}

# Cross-compile, install atomically on rpi1, restart, and verify its health endpoint.
pi-deploy: device-build
    ./scripts/deploy-device.sh

# Show systemd's current Pi service status.
pi-status:
    ssh -4 -F /dev/null -o BatchMode=yes {{pi_host}} 'systemctl --no-pager --full status daily-mirror-device.service'

# Show recent Pi service logs.
pi-logs:
    ssh -4 -F /dev/null -o BatchMode=yes {{pi_host}} 'journalctl -u daily-mirror-device.service -n 100 --no-pager'

# Read the Pi service health and software version.
pi-health:
    curl --fail --silent --show-error {{pi_admin}}/healthz
    @echo

# Verify generated clients and tests before a Vercel deployment.
deploy-check: check
    cd server && test -x node_modules/.bin/vercel
    cd server && test -f .vercel/project.json || { echo "server is not linked; run: cd server && ./node_modules/.bin/vercel link" >&2; exit 1; }
    cd server && test -x scripts/deploy-prebuilt.sh
    cd server && test ! -f vercel.json || { echo "server/vercel.json shadows the generated .nextrs/vercel.json; move settings into [vercel] in nextrs.toml" >&2; exit 1; }
    cd server && nextrs bundles plan > /dev/null
    bash -n scripts/deploy-device.sh
    bash -n server/scripts/deploy-prebuilt.sh

# Build locally and deploy the server to the linked Vercel production project.
deploy: deploy-check
    cd server && PATH="node_modules/.bin:$PATH" ./scripts/deploy-prebuilt.sh

# Build locally and deploy an unaliased Vercel preview.
deploy-preview: deploy-check
    cd server && PATH="node_modules/.bin:$PATH" ./scripts/deploy-prebuilt.sh --preview

# Verify a deployed server. Example: just server-health https://example.vercel.app
server-health server_url:
    curl --fail --silent --show-error "{{server_url}}/healthz"
    @echo
