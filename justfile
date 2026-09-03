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

# Regenerate the typed web client after changing a Rust API route.
client:
    cd server && npm run client:generate

# Format, compile, type-check, and test both Rust applications.
check:
    cd device && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd processor && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
    cd server && npm run client:generate && cargo fmt --check && cargo clippy --all-targets -- -D warnings -A clippy::match-single-binding && npm run typecheck && cargo test
    git diff --check

# Show the pending, leased, complete, and failed face-processing counts.
process-status:
    cd processor && cargo run --locked -- status

# Drain face-processing work. This refuses to claim until an inference engine is configured.
process:
    cd processor && cargo run --locked -- process

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
    cd server && test -f api/index.rs && test -f vercel.json && test -x scripts/deploy-prebuilt.sh
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
