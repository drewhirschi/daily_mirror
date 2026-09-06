//! Measure admin data reads without printing private photo or identity data.
use server::{catalog::PhotoCatalog, processing::ProcessingQueue};
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let queue = ProcessingQueue::new(PhotoCatalog::from_env()?);
    for run in 1..=2 {
        let start = Instant::now();
        let dashboard = queue.admin_dashboard().await?;
        println!(
            "Dashboard run {run}: {} ms, {} photos",
            start.elapsed().as_millis(),
            dashboard.photos.len()
        );
        let start = Instant::now();
        let people = queue.people_with_flipbooks().await?;
        println!(
            "People run {run}: {} ms, {} people",
            start.elapsed().as_millis(),
            people.people.len()
        );
    }
    Ok(())
}
