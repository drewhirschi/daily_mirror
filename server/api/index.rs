// @generated deployment adapter for Vercel's required api/index.rs entry.
//
// Do not put application logic here: src/app.rs owns the shared Router. If
// this project will not deploy to Vercel, remove this file together with the
// `index` Cargo target, Vercel-only dependencies, and vercel.json.
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(server::app());

    vercel_runtime::run(app).await
}
