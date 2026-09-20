# Daily Mirror deployment

Current production origin: `https://daily-mirror-pearl.vercel.app`. The Pi is
provisioned with this origin and a bearer token shared only with Vercel. The
private R2 bucket is `daily-mirror`, under the `photos/` key prefix.

The production data path is intentionally split:

1. The Pi retains each capture in its local durable queue.
2. It authenticates to the NextRS server and requests `POST /api/uploads`.
3. The server returns a five-minute, single-object R2 `PUT` URL.
4. The Pi uploads the JPEG directly to R2, then calls the server's completion
   endpoint. The server verifies the object size and marks the Turso row ready.
5. The Pi removes its queued copy only after completion succeeds.
6. The gallery queries Turso through the server. Image requests are
   redirected to short-lived signed R2 `GET` URLs.

Vercel therefore handles only small JSON requests, never the multi-megabyte
photo body. Capture IDs are also object keys, so a retry overwrites the same
object instead of creating a duplicate.

## One-time Cloudflare setup

Create a private R2 bucket (for example `daily-mirror`) and an R2 API token
scoped to that bucket with object read/write and bucket-list access. Record the
account-specific S3 endpoint, access key ID, and secret access key. The secret
is shown only when the token is created.

The Pi does not need R2 credentials. It receives only expiring URLs for one
specific capture ID. Browser CORS configuration is unnecessary because uploads
come from the Pi process and gallery reads navigate to the signed URL.

## One-time Vercel setup

Install dependencies and link the `server/` directory to a Vercel project:

```sh
just install
cd server
./node_modules/.bin/vercel login
./node_modules/.bin/vercel link
```

Add these Production and Preview environment variables in Vercel:

```text
DAILY_MIRROR_STORAGE_BACKEND=r2
DAILY_MIRROR_DATABASE_URL=libsql://daily-mirror-<org>.turso.io
DAILY_MIRROR_DATABASE_AUTH_TOKEN=<Turso database token>
DAILY_MIRROR_UPLOAD_TOKEN=<long random device token>
DAILY_MIRROR_AUTH_ORIGIN=https://daily-mirror-pearl.vercel.app
DAILY_MIRROR_R2_ENDPOINT=https://<account-id>.r2.cloudflarestorage.com
DAILY_MIRROR_R2_BUCKET=daily-mirror
DAILY_MIRROR_R2_ACCESS_KEY_ID=<R2 access key>
DAILY_MIRROR_R2_SECRET_ACCESS_KEY=<R2 secret key>
DAILY_MIRROR_R2_PREFIX=photos
DAILY_MIRROR_R2_PRESIGN_SECONDS=300
DAILY_MIRROR_R2_URL_STYLE=virtual-host
CRON_SECRET=<at least 16 random characters>
```

Do not put these secrets in a checked-in `.env` file. Local development stays
on `DAILY_MIRROR_STORAGE_BACKEND=local`. Production refuses to start without
the device upload token or a canonical HTTPS authentication origin. Create the
first gallery account against the same Turso database before production goes
live:

```sh
just auth-create-user drew
```

The CLI prompts twice and never places the password in shell history. Gallery
login creates a 30-day session; a signed-in user can enroll passkeys from the
Account page. `/healthz` and the bearer-authenticated device upload path remain
available without a gallery session.

The `#[nextrs::cron]` attribute on the route schedules
`GET /api/maintenance/reconcile` once per day at 09:00 UTC, emitted into the
generated `.nextrs/vercel.json` at deploy time. Vercel sends `CRON_SECRET` as a Bearer authorization header. The
server rejects missing or incorrect secrets, lists R2, repairs complete
`pending` uploads, inserts objects that have no catalog row, and reports—but
does not automatically delete—catalog rows whose objects are missing. The job
is idempotent and is registered only by production deployments.

Cloudflare-provider crons are shipped by `nextrs cron deploy`, which reads
`CRON_SECRET` — and `CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` when the
API transport is used — from the process environment only; the CLI reads no env
file. `server/scripts/deploy-prebuilt.sh` therefore exports just those three
variables from `server/.vercel/.env.production.local` (written by `vercel pull`)
before that step, parsing the file rather than sourcing it and never printing a
value. A variable already exported in the shell wins over the file. If the file
is missing the script warns, suggests the command below, and continues:

```sh
cd server && vercel env pull .vercel/.env.production.local --environment=production
```

This is a workaround for a nextrs limitation and can be deleted once the CLI
loads the pulled env file itself.

## Database schema and migrations

The schema is defined once, by the ordered `.sql` files in
`server/migrations/`. Nothing on a request path creates or alters a table any
more: the server only *checks* that the database is at the version its binaries
expect, and `server/scripts/deploy-prebuilt.sh` applies pending migrations to
production **before** the new build goes live.

```sh
cd server
cargo run --bin daily-mirror-migrate -- status        # read-only
cargo run --bin daily-mirror-migrate -- up --dry-run  # what `up` would run
cargo run --bin daily-mirror-migrate -- up            # apply
```

The binary reads `DAILY_MIRROR_DATABASE_URL` and
`DAILY_MIRROR_DATABASE_AUTH_TOKEN` exactly as the other bins do. With no URL
it uses the local `server/data/daily-mirror.db`. Applied migrations are
recorded in `schema_migrations` (version, name, applied_at, checksum), and a
single-row `schema_migrations_lock` keeps two runners from interleaving.

### How the deploy gets production credentials

Vercel does not pull variables marked sensitive, so
`.vercel/.env.production.local` holds a `[SENSITIVE]` placeholder where the
database token should be. The deploy script therefore, in order:

1. uses `DAILY_MIRROR_DATABASE_URL` and `DAILY_MIRROR_DATABASE_AUTH_TOKEN` if
   both are already exported;
2. otherwise takes the URL from `.vercel/.env.production.local` (that one *is*
   pulled) and mints a one-hour token with the Turso CLI
   (`~/.turso/turso db tokens create <database> --expiration 1h`), deriving the
   database name from the URL host — set `DAILY_MIRROR_TURSO_DB` to override;
3. otherwise stops with an explanation and deploys nothing.

The token is never printed and never written to disk. A preview deploy
(`just deploy-preview`) skips migrations entirely.

### Failing closed

If the database is behind the running binary — or its recorded history does
not match the binary's migration files — every `/api/` route answers `503`
with a machine-readable body:

```json
{ "current_version": 1, "expected_version": 2, "pending": 1,
  "code": "schema_migration_pending", "detail": "..." }
```

`/healthz` stays reachable and reports the same block under `schema`, with
`status: "degraded"`. Codes are `schema_migration_pending`,
`schema_migration_drift` and `schema_unreadable`.

### Writing a migration

Vercel keeps serving the *previous* build until the new one is live, and
migrations run before that switchover. The old code therefore runs against the
new schema for a minute or two, which gives one hard rule:

> **Migrations are expand-only.** Add nullable columns, new tables and new
> indexes. Never drop or rename anything in the same release that stops using
> it — remove it a release later, once nothing deployed reads it.

The checklist:

1. Add `server/migrations/NNNN_short_name.sql`, numbered one past the last.
2. Keep every statement idempotent: `CREATE TABLE IF NOT EXISTS`,
   `CREATE INDEX IF NOT EXISTS`, or `ALTER TABLE ... ADD COLUMN` — the runner
   deliberately ignores "duplicate column name" so an interrupted migration can
   simply be re-run. No `DROP`, no `NOT NULL` without a `DEFAULT`.
3. Register it in `MIGRATIONS` in `server/src/migrations.rs`.
4. Never edit a migration that has shipped. Its checksum is recorded, and a
   changed file is reported as drift and refused.
5. `cargo test` — the suite covers a fresh database, adoption of a
   production-shaped one, drift detection and the fail-closed behaviour.
6. Deploy. `just deploy` migrates production, then uploads the build.

Migration `0001_baseline` is the schema as it stood before migrations existed.
It is written to succeed against both an empty database and today's
production, which already has every object and no `schema_migrations` table —
the first production run simply records what is already there.

## Deploy and verify

```sh
just deploy-preview
just server-health https://<preview-url>
just deploy
just server-health https://<production-url>
```

`just deploy` applies pending schema migrations to production before it
uploads the build, and stops without deploying if they fail. To see what it
would do first:

```sh
cd server && cargo run --bin daily-mirror-migrate -- status
```

`just server-health` now also reports the schema version and pending count.

After production is healthy, set the Pi's `DAILY_MIRROR_SERVER_URL` to the
production origin and keep its `DAILY_MIRROR_UPLOAD_TOKEN` equal to the Vercel
value. Deploy the updated device binary with `just pi-deploy`. Any photo already
in the Pi queue will use the same signed-upload flow on its next retry.

## Routine commands

```sh
just                 # list commands
just check           # device + server tests and web type-check
just device-build    # cross-compile for the 64-bit Pi
just pi-deploy       # install, restart, and wait for /healthz
just pi-health       # live Pi version and status
just pi-status       # systemd status
just pi-logs         # latest service logs
```

## Second Pi (rpi2)

Provisioned 2026-09-08 at `drew@rpi2.local`, admin URL
`http://rpi2.local:8081`, using the same service layout and production upload
credentials as rpi1. Its `.env` selects `rgb-common-anode`, button GPIO2,
green GPIO17, blue GPIO27 (legacy `YELLOW_LED_PIN` setting), and red GPIO22.
Normal camera arguments omit IMX519 autofocus options. Camera identification
and capture validation are pending: `rpicam-still --list-cameras` reported
`No cameras available!` during provisioning. The user reports an older Pi
camera, possibly Camera Rev 1.3; this is not yet confirmed. No camera overlay
changes were made. Health's `camera_available` currently checks the executable,
not sensor detection; do not treat that field as a passed capture test.

Target this Pi explicitly for future deployments (the defaults remain rpi1):

```sh
DAILY_MIRROR_PI_HOST=drew@rpi2.local \
DAILY_MIRROR_PI_ADMIN_URL=http://rpi2.local:8081 just pi-deploy
```

The unit is enabled at boot. GPIO initialization and the yellow/restore admin
commands were exercised successfully; visible LED colors and the physical
button still need user confirmation. No test photo/upload has been verified.

### rpi2 development mode (supersedes initial upload setup)

rpi2 now uses `DAILY_MIRROR_CAPTURE_MODE=local` and a provisional
`DAILY_MIRROR_CAMERA_PROFILE=ov5647`. Its production URL and upload token have
been removed. Normal captures are stored in `/home/drew/daily-mirror-device/data/local`
and can be viewed on its admin page. They never enter the upload retry queue.
Sensor detection remains unresolved; confirm the camera revision and ribbon
connection before treating the profile as hardware-verified.

### Camera detection resolved

The runtime `ov5647` overlay successfully detected the sensor on rpi2 and a
normal admin Capture action saved a valid 2592 × 1944 JPEG locally. No upload
was attempted. The OV5647 profile is now confirmed, superseding the provisional
identification above. `/boot/firmware/config.txt` was updated to
`camera_auto_detect=0` plus `dtoverlay=ov5647` in an `[all]` section. Original
configuration backup: `/boot/firmware/config.txt.before-daily-mirror-camera-20260909T023654Z`.
The working runtime overlay was retained. No reboot was needed for the test;
a future reboot is still needed to verify persistence end to end.
