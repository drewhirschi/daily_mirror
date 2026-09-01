# Daily Mirror server

The NextRS web application for Daily Mirror. Rust owns the HTTP API and local
photo storage; React renders the private chronological gallery.

## Start developing

Install the CLI once, then use any of the equivalent dev commands:

```sh
cargo install cargo-nextrs
cargo dev
# cargo nextrs dev
# nextrs dev
```

`cargo dev` refreshes the generated client before starting the watcher. Run
`cargo nextrs client generate` explicitly after changing an API when you only
want to refresh types. Install JavaScript dependencies only at this project
root—never inside `.nextrs/`.

## Run locally

```sh
cp .env.example .env
npm install
npm run client:generate
cargo dev
```

Open <http://localhost:3000>. Local photo files default to `data/photos` and
are excluded from Git.

## Device upload contract

The device first requests a short-lived upload target:

```http
POST /api/uploads
Content-Type: application/json
Authorization: Bearer <DAILY_MIRROR_UPLOAD_TOKEN>

{
  "capture_id": "20260828T210000Z-ab12cd34",
  "content_type": "image/jpeg",
  "content_length": 2270779
}
```

For local storage, the response points back to the existing raw upload route.
With R2 storage, it contains a presigned `PUT` URL and the exact headers the
device must send. The R2 access key and secret never leave the server.

The local/raw target remains:

```http
POST /api/photos
Content-Type: image/jpeg
X-Capture-Id: 20260828T210000Z-ab12cd34
Authorization: Bearer <DAILY_MIRROR_UPLOAD_TOKEN>
```

The body is the raw JPEG. Capture IDs make retries idempotent. The server
rejects malformed IDs, incomplete JPEGs, and bodies larger than 32 MiB. The Pi
deletes its durable queued file only after the target accepts the JPEG. When
`DAILY_MIRROR_UPLOAD_TOKEN` is omitted, unauthenticated uploads are allowed for
local development only; R2 mode refuses to start without a token.

For R2, accepting the JPEG is a three-step durable protocol. The server first
reserves a `pending` catalog row and returns a short-lived, object-scoped R2
URL. After the PUT succeeds, the device calls the server's completion endpoint;
the server verifies the stored object and its byte size before marking the row
`ready`. The device retains its local queued JPEG unless completion succeeds,
so retrying the entire exchange is safe and idempotent. Database and permanent
R2 credentials remain on the server; the device receives neither.

`GET /api/photos` queries the libSQL photo catalog newest-first by capture time.
Each ready catalog entry includes a versioned 320-pixel WebP thumbnail URL;
`GET /api/photos/:id` returns the original only when the viewer opens it. The
gallery also enforces capture-time ordering client-side as a defensive measure.
Escape, the close button, or clicking the backdrop dismisses the viewer.

## Storage

`src/photos.rs` is the binary storage boundary. Local mode writes one JPEG per capture
to `DAILY_MIRROR_STORAGE_DIR` using a temporary file followed by an atomic
rename. R2 mode signs direct device uploads, lists the private bucket through
its S3-compatible API, and redirects image reads to short-lived signed URLs.
`src/catalog.rs` stores image IDs, object keys, capture times, byte sizes,
upload state, and applied rotation in Turso/libSQL. Local development defaults
to `data/daily-mirror.db`. Production uses `DAILY_MIRROR_DATABASE_URL` and
`DAILY_MIRROR_DATABASE_AUTH_TOKEN`. Gallery reads query only the database.
Upload completion creates the thumbnail and marks the row ready; maintenance
repairs stranded uploads, orphaned originals, and missing thumbnails without
putting storage scans on the interactive gallery path.

Production also runs an authenticated daily reconciliation at
`GET /api/maintenance/reconcile`. It repairs R2 objects missing catalog rows,
completed uploads stranded in `pending`, and missing thumbnails, while leaving
size mismatches hidden for investigation. `CRON_SECRET` stays in Vercel; Vercel
supplies it as a Bearer token when invoking the schedule declared in
`vercel.json`.

Set the variables documented in `.env.example`; R2 API credentials need object
read/write plus bucket-list access.

## Health

`GET /healthz` returns the server software version and active storage backend.
It is used by the deployment recipes and is intentionally lightweight.

## Gallery accounts and passkeys

The gallery uses named accounts rather than HTTP Basic authentication. Create
an account from the repository root; the command prompts twice without putting
the password in shell history:

```sh
just auth-create-user drew
```

Passwords must be at least 12 characters and are stored as Argon2id hashes.
Successful password or passkey login mints a random 30-day session cookie; only
its SHA-256 hash is stored in the database. From **Account**, a signed-in user
can enroll one or more passkeys and use them on later visits. Passwords remain
the recovery path. Passkey challenges are short-lived, single-use, and stored
server-side so a replayed ceremony cannot create a session. Password failures
are durably throttled per account and client after eight attempts in 15 minutes.

Set `DAILY_MIRROR_AUTH_ORIGIN` to the canonical HTTPS production URL before
deploying. Temporary Vercel previews derive their origin from `VERCEL_URL`.
`DAILY_MIRROR_AUTH_RP_ID` is optional and normally derives from the origin host.
`/healthz` and bearer-authenticated device writes bypass gallery authentication.

## Install on iPhone or iPad

Daily Mirror includes an app manifest, Apple home-screen icon, standalone
display metadata, and a network-only service worker. In iOS, open the production
site, tap **Share**, choose **Add to Home Screen**, turn on **Open as Web App**,
then tap **Add**. iOS gives the installed app separate cookies and storage, so it
may require one sign-in after installation; that app session then lasts 30 days.
The service worker deliberately does not cache gallery HTML or photographs, so
private archive data is not copied into a separate offline cache.

## Build and deploy

From the repository root, `just` lists the supported workflows. The common
ones are:

```sh
just check
just dev
just deploy-preview
just deploy
just server-health https://your-project.vercel.app
```

The Vercel recipes use the generated prebuilt deployment path: Rust and the web
bundle compile on this computer, the function artifact is verified, and only
then is it uploaded. Git-triggered Vercel builds are disabled in `vercel.json`.
See `docs/deployment.md` at the repository root for the one-time R2 and Vercel
setup.

## Run in a container

```sh
export DAILY_MIRROR_UPLOAD_TOKEN=choose-a-local-token
docker compose up --build
```

The app is available on <http://localhost:3000>. Photos persist in
`server/data/`, mounted at `/app/data` inside the container.

## Project structure

- `app/`: URL routes and code used by one route. Only convention filenames
  such as `page.tsx`, `layout.tsx`, `prefetch.rs`, and `route.rs` create routes;
  a colocated `TodoRow.tsx` is an ordinary component.
- `components/`: React components shared by multiple routes.
- `src/`: Rust application and domain logic. `src/app.rs` constructs the shared
  Router; `src/main.rs` is the local process entry point.
- `public/`: static files.
- `.nextrs/`: generated framework state. Do not edit it.
- `api/index.rs`: generated Vercel adapter. It contains no application logic.

## Generated API client

A `#[nextrs::api]` Rust handler is exposed through a genuine linked TypeScript
package. Plain fetch functions and React Query integration have separate entry
points:

The gallery imports `useGetApiPhotos` from `@server/client/react-query`. After
changing a `#[nextrs::api]` route, regenerate the package with:

```sh
npm run client:generate
```

TypeScript and editors resolve both through root `node_modules`; no declaration
shim, relative generated path, or `tsconfig.paths` entry is required.
