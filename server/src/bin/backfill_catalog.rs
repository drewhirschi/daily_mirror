use std::io;

use server::{catalog::PhotoCatalog, photos::PhotoStore};

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::dotenv().ok();
    let store = PhotoStore::from_env()?;
    let catalog = PhotoCatalog::from_env()?;
    let stored = store.list().await?;
    let records = stored
        .into_iter()
        .map(|photo| {
            let storage_key = store.storage_key(&photo.id)?;
            Ok((photo, storage_key))
        })
        .collect::<io::Result<Vec<_>>>()?;

    catalog.import(&records).await?;
    let catalog_count = catalog.list().await?.len();
    println!(
        "Backfill complete: {} R2 object(s), {} ready catalog row(s)",
        records.len(),
        catalog_count
    );
    if records.len() != catalog_count {
        return Err(io::Error::other("R2 and catalog counts do not match"));
    }
    Ok(())
}
