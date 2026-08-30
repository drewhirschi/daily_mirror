# Daily Mirror deployment

Current production origin: `https://daily-mirror-pearl.vercel.app`. The Pi is
provisioned with this origin and a bearer token shared only with Vercel. The
private R2 bucket is `daily-mirror`, under the `photos/` key prefix.

The production data path is intentionally split:

1. The Pi retains each capture in its local durable queue.
2. It authenticates to the NextRS server and requests `POST /api/uploads`.
3. The server returns a five-minute, single-object R2 `PUT` URL.
4. The Pi uploads the JPEG directly to R2 and removes its queued copy only
   after a successful response.
5. The gallery lists the private bucket through the server. Image requests are
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
DAILY_MIRROR_VIEW_USERNAME=daily-mirror
DAILY_MIRROR_VIEW_PASSWORD=<long random gallery password>
DAILY_MIRROR_R2_ENDPOINT=https://<account-id>.r2.cloudflarestorage.com
DAILY_MIRROR_R2_BUCKET=daily-mirror
DAILY_MIRROR_R2_ACCESS_KEY_ID=<R2 access key>
DAILY_MIRROR_R2_SECRET_ACCESS_KEY=<R2 secret key>
DAILY_MIRROR_R2_PREFIX=photos
DAILY_MIRROR_R2_PRESIGN_SECONDS=300
DAILY_MIRROR_R2_URL_STYLE=virtual-host
```

Do not put these secrets in a checked-in `.env` file. Local development stays
on `DAILY_MIRROR_STORAGE_BACKEND=local`. Production refuses to start without
both the device upload token and gallery password. The browser presents a
standard Basic-auth login prompt; `/healthz` and the bearer-authenticated device
upload path remain available without gallery credentials.

## Deploy and verify

```sh
just deploy-preview
just server-health https://<preview-url>
just deploy
just server-health https://<production-url>
```

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
