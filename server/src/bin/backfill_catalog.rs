use std::io;

use server::{
    catalog::PhotoCatalog,
    photos::PhotoStore,
    processing::{ProcessingQueue, active_pipeline_version},
    upload_flow::reconcile_all,
};

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::dotenv().ok();
    let store = PhotoStore::from_env()?;
    let catalog = PhotoCatalog::from_env()?;
    let processing = ProcessingQueue::new(catalog.clone());
    let report = reconcile_all(&store, &catalog).await?;
    let queued = processing
        .reconcile_missing(&active_pipeline_version()?)
        .await?;
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
    println!("Queued {queued} photo(s) missing processing state");
    if report.thumbnail_failures > 0 || report.missing_storage_objects > 0 {
        return Err(io::Error::other(
            "reconciliation finished with unresolved media",
        ));
    }
    Ok(())
}
