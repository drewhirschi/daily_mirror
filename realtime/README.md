# Daily Mirror household realtime proof of concept

This is a Rust [`workers-rs`](https://github.com/cloudflare/workers-rs) Worker.
It maps each household ID to one Cloudflare Durable Object. The object accepts
hibernatable WebSockets, persists an event sequence, and broadcasts small JSON
notifications to every gallery currently connected to that household.

The Worker is deliberately not the photo database or image store. Vercel,
libSQL, and R2 remain authoritative. Realtime delivery is best effort and tells
the browser to refetch the catalog; the gallery's one-minute poll is retained
as a repair path.

## Event path

1. An authenticated gallery asks the Vercel app for `/api/realtime/session`.
2. The Rust server mints a 60-second HMAC ticket scoped to its configured
   household. The browser never sees the publish credential.
3. The browser opens `/v1/households/:id/connect?ticket=...` on this Worker.
4. A successful photo mutation posts `photo.created`, `photo.updated`,
   `photo.deleted`, or `photos.reconciled` with the server-only publisher token.
5. The household object adds a durable sequence and broadcasts the event. The
   sample gallery client refetches `/api/photos`.

The transport accepts other event names too. For example,
`household.notice` with `{ "message": "Camera is offline" }` is rendered by the
sample client, demonstrating non-photo household updates.

## Local setup

Install the Rust build adapter and Wrangler, then copy the development secrets:

```sh
cargo install worker-build --version 0.8.5 --locked
npm install
cp .dev.vars.example .dev.vars
npm run dev
```

Use the same ticket secret and publisher token in `server/.env`, and configure
the gallery origin in `wrangler.jsonc`:

```dotenv
DAILY_MIRROR_REALTIME_URL=http://127.0.0.1:8787
DAILY_MIRROR_HOUSEHOLD_ID=home
DAILY_MIRROR_REALTIME_TICKET_SECRET=<same value as TICKET_SECRET>
DAILY_MIRROR_REALTIME_PUBLISH_TOKEN=<same value as PUBLISH_TOKEN>
```

Then start the NextRS server normally. Leaving `DAILY_MIRROR_REALTIME_URL`
unset disables the proof of concept cleanly.

## Verify and deploy

```sh
cargo test
cargo clippy --target wasm32-unknown-unknown -- -D warnings
worker-build --release
npm run test:e2e
npx wrangler secret put TICKET_SECRET
npx wrangler secret put PUBLISH_TOKEN
npm run deploy
```

After deployment, set `ALLOWED_ORIGINS` in `wrangler.jsonc` to the canonical
gallery origin, set `DAILY_MIRROR_REALTIME_URL` in Vercel to the Worker's HTTPS
URL, and install the matching secrets and household ID in Vercel.

## celld portability

The implementation stays within celld's documented Cloudflare-compatible
surface: Wrangler configuration, Durable Objects, hibernatable inbound
WebSockets, storage, and Web Crypto-compatible HMAC tickets. R2 is not required
by this Worker. The same built Worker can therefore be used for a celld
experiment while the existing Vercel/R2 application remains unchanged.

This branch intentionally models the current app as one configured household.
A production multi-household version still needs household membership in the
main database, authorization against that membership before ticket minting,
key rotation, rate limits, and observability.
