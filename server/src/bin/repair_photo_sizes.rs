use std::io;

use server::{catalog::PhotoCatalog, photos::PhotoStore};

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();

    let ids = std::env::args().skip(1).collect::<Vec<_>>();
    if ids.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: repair_photo_sizes <photo-id>...",
        ));
    }

    let store = PhotoStore::from_env()?;
    let catalog = PhotoCatalog::from_env()?;

    for id in ids {
        let expected = catalog.expected_size(&id).await?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("photo {id} is not cataloged"),
            )
        })?;
        let actual = store.uploaded_size(&id).await?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("photo {id} is not stored"))
        })?;

        if expected == actual {
            println!("{id}: already correct ({actual} bytes)");
            continue;
        }

        catalog.repair_ready_size(&id, actual).await?;
        println!("{id}: repaired {expected} -> {actual} bytes");
    }

    Ok(())
}
