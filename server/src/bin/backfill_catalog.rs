use std::io;

use server::{catalog::PhotoCatalog, photos::PhotoStore, upload_flow::reconcile_all};

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::dotenv().ok();
    let store = PhotoStore::from_env()?;
    let catalog = PhotoCatalog::from_env()?;
    let report = reconcile_all(&store, &catalog).await?;
    println!(
        "Reconciliation complete: {} storage object(s), {} ready row(s), {} recovered upload(s), {} imported orphan(s), {} generated thumbnail(s), {} thumbnail failure(s), {} missing original(s)",
        report.storage_objects,
        report.ready_after,
        report.recovered_pending,
        report.imported_orphans,
        report.generated_thumbnails,
        report.thumbnail_failures,
        report.missing_storage_objects,
    );
    if report.thumbnail_failures > 0 || report.missing_storage_objects > 0 {
        return Err(io::Error::other(
            "reconciliation finished with unresolved media",
        ));
    }
    Ok(())
}
