# Daily Mirror

Daily Mirror is a small physical camera appliance: press one button, take one
photograph, and upload it to a private chronological gallery.

The current revival keeps the interaction deliberately narrow. There is no
identity recognition, comparison, notes, alignment, or time-lapse generation.

## Repository layout

- `device/` — new Rust Raspberry Pi client: camera command, GPIO button and
  three LEDs, durable upload queue, and retries.
- `server/` — new NextRS/Axum API, local filesystem storage, React gallery,
  and Docker Compose configuration.
- `processor/` — intermittent Rust face-processing CLI and leased batch runner;
  the inference adapter is intentionally pending model selection.
- `crates/vision-contract/` — shared queue and face-result wire types used by
  the server and processor.
- `src/`, `snapr/`, and `view/` — the original 2022 Python, Rust/OpenCV, and
  React prototypes retained for reference.
- `data/` — photographs and face-recognition experiments from the original
  prototype. New captures do not belong in Git.

## Prototype flow

```text
button -> LED countdown -> ArduCam JPEG -> durable Pi queue
       -> signed upload grant -> local storage or private R2 -> web gallery
```

The device writes a completed JPEG to its queue before uploading. It removes
that file only after the server acknowledges it. A stable capture ID makes
network retries idempotent.

## Start the web server

```sh
cd server
cp .env.example .env
npm install
npm run client:generate
cargo dev
```

See `server/README.md` for the upload API and container setup.

The password-protected production gallery is deployed at
<https://daily-mirror-pearl.vercel.app>.

The planned path from breadboard prototype to an enclosed Pi appliance, R2 and
Vercel deployment, and a later ESP32 port is documented in
[`docs/productization-plan.md`](docs/productization-plan.md).
Deployment commands and the direct-to-R2 upload flow are documented in
[`docs/deployment.md`](docs/deployment.md).
The local Rust processing worker, durable job queue, face landmarks, identity
review workflow, and centered portrait pipeline are documented in
[`docs/face-processing-architecture.md`](docs/face-processing-architecture.md).

## Bring up the Raspberry Pi camera

```sh
cd device
cp .env.example .env
cargo run --release -- capture-once --no-upload
```

After validating the ArduCam output, configure the server URL and test an
upload before wiring GPIO. See `device/README.md` for pin assignments and LED
behavior.

## Inspect the face-processing queue

After setting `DAILY_MIRROR_SERVER_URL`, `DAILY_MIRROR_PROCESSOR_TOKEN`, and
`DAILY_MIRROR_PROCESSING_PIPELINE` in `processor/.env`, run:

```sh
just process-status
```

The leased Rust processing loop is implemented, but `just process` will refuse
to claim production photos until a face inference engine is selected. See
[`processor/README.md`](processor/README.md) and the
[face-processing architecture](docs/face-processing-architecture.md).

## Quality gates

Run `just check` to format-check, lint, generate and validate the typed client,
type-check the gallery, and test all Rust applications. Run
`just install-hooks` once per clone to make the same suite a mandatory local
pre-push hook. GitHub Actions runs it again for every push and pull request,
and production deployment refuses to start until it passes.
