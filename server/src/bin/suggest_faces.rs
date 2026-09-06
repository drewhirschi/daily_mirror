//! Refresh identity suggestions using the configured catalog, without reprocessing images.
use server::{catalog::PhotoCatalog, processing::ProcessingQueue};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let queue = ProcessingQueue::new(PhotoCatalog::from_env()?);
    let result = queue.refresh_face_suggestions().await?;
    println!(
        "Examined {} unconfirmed faces; {} proposed matches.",
        result.examined, result.proposed
    );
    Ok(())
}
