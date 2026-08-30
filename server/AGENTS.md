# server — contract for coding agents

This is a [nextrs](https://nextrs-docs.vercel.app/docs/getting-started) app:
Rust (Axum) serving Next.js-style file routes with React `.tsx` pages. The
scaffold generated the wiring below — treat it as framework, not app code.

User code belongs in `app/`, `components/`, and `src/`. `.nextrs/` is
framework-owned generated state; import its linked package, never edit it.

## The app/ tree is the router

Directories containing recognized convention files contribute URL segments.
Ordinary `.ts`/`.tsx` modules may be colocated beside a page without creating a
route; put components shared across routes in top-level `components/`.

| File | Role |
|---|---|
| `page.{tsx,rs,html}` | The content for this URL (`.tsx` = client-rendered React) |
| `layout.tsx` or `layout.rs` + `layout.html` | Wraps this segment's children (Askama layouts need `{{ children|safe }}`) |
| `loading.{tsx,rs,html}` | Skeleton streamed while the page computes |
| `middleware.rs` | Guard, runs before anything renders |
| `route.rs` | API handlers — one `pub async fn get/post/...` per method, `#[nextrs::api]` for the typed client |
| `prefetch.rs` | Server data seeding a `page.tsx`'s React Query cache (requires the `.tsx` sibling) |

A `.tsx` slot is exclusive: it cannot coexist with `.rs`/`.html` of the same
name. Full reference: <https://nextrs-docs.vercel.app/docs/conventions>

## Never hand-roll what the scaffold generates

`build.rs`, `src/main.rs`, `api/index.rs`, `vercel.json`,
`scripts/deploy-prebuilt.sh`, and `.nextrs/` are generated wiring. Never edit
generated output under `.nextrs/` or `public/dist/`; application seams are
`app/**`, `components/**`, and `src/**`. `src/app.rs` is the shared Rust app,
while `src/main.rs` and `api/index.rs` are process adapters.

## The client package and the bare-import rule

`.nextrs/client` is a real npm workspace package; pages import it as
`@server/client` or `@server/client/react-query`.

- **Ignore all of `.nextrs/client`.** It is generated state. The tracked
  `.nextrs/template/client` wiring recreates the package before generation;
  never commit or hand-edit its contents.
- **Every bare import used by any `.tsx` file belongs in the root
  `package.json`**. Run `npm install` only at the app root; never install
  dependencies inside `.nextrs/client`.
- **Never hand-write API types.** After changing `#[nextrs::api]` routes, run
  `cargo nextrs client generate` at the app root. The Cargo command owns the
  OpenAPI, Orval, declaration, and package build steps.
  Guide: <https://nextrs-docs.vercel.app/docs/typesafe-client>

## Dev loop

```bash
cargo dev   # build + run + watch (`cargo install cargo-nextrs` once)
```

Don't substitute a hand-rolled watch script — the runner knows which inputs
(Rust, templates, `app/`, `public/`, env files) require a restart.

## Diagnosing a slow route

Every response carries a `Server-Timing` breakdown — read it before adding
any logging:

```bash
curl -sI http://localhost:3000/todos | grep -i server-timing
# server-timing: mw;dur=1.2, seed;dur=430.0, handler;dur=445.1, total;dur=447.0, route;desc="/todos"
```

`mw` = middleware chain, `seed` = `prefetch.rs` data seeding, `handler` =
page render or API fn. When `handler` is the mystery, extract
`nextrs::Timing` and wrap the suspects — the segment appears in the same
header on the next request:

```rust
pub async fn get(timing: nextrs::Timing, Extension(db): Extension<Db>) -> Json<Vec<Todo>> {
    let todos = timing.span("db", db.list()).await;
    Json(todos)
}
```

The same data fires as `tracing` events (`RUST_LOG=nextrs=info` locally;
Vercel function logs in production). Full guide, including OpenTelemetry
export: <https://nextrs-docs.vercel.app/docs/telemetry>

## Deploys are prebuilt

Git auto-builds are OFF (`vercel.json` sets `git.deploymentEnabled: false`);
pushing deploys nothing. The deploy path is:

```bash
scripts/deploy-prebuilt.sh             # production
scripts/deploy-prebuilt.sh --preview   # preview
```

Guide: <https://nextrs-docs.vercel.app/docs/deploy-prebuilt>

## Porting into this app

Bringing routes over from an existing app? Graft them into this skeleton —
`route.ts` bodies become `route.rs` handlers, auth becomes `middleware.rs`,
React pages drop into `app/**/page.tsx` — rather than assembling parallel
structure around it. The paved road, including the strangler pattern for
incremental conversion and the gotchas list:
<https://nextrs-docs.vercel.app/docs/porting>
