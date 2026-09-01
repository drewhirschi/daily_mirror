use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::catalog::PhotoCatalog;
use crate::photos::{Photo, PhotoStore};

#[derive(Debug)]
pub enum FinalizeError {
    InvalidCaptureId(io::Error),
    ReservationNotFound,
    ObjectNotFound,
    SizeMismatch { expected: u64, actual: u64 },
    Catalog(io::Error),
    Storage(io::Error),
}

impl fmt::Display for FinalizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCaptureId(error) => write!(formatter, "invalid capture ID: {error}"),
            Self::ReservationNotFound => formatter.write_str("upload reservation not found"),
            Self::ObjectNotFound => formatter.write_str("uploaded object not found"),
            Self::SizeMismatch { expected, actual } => write!(
                formatter,
                "uploaded object size mismatch: expected {expected}, got {actual}"
            ),
            Self::Catalog(error) => write!(formatter, "photo catalog failed: {error}"),
            Self::Storage(error) => write!(formatter, "photo storage failed: {error}"),
        }
    }
}

impl std::error::Error for FinalizeError {}

impl FinalizeError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidCaptureId(_) => StatusCode::BAD_REQUEST,
            Self::ReservationNotFound => StatusCode::NOT_FOUND,
            Self::ObjectNotFound | Self::SizeMismatch { .. } => StatusCode::CONFLICT,
            Self::Storage(_) => StatusCode::BAD_GATEWAY,
            Self::Catalog(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReconcileReport {
    pub storage_objects: usize,
    pub ready_before: usize,
    pub ready_after: usize,
    pub recovered_pending: usize,
    pub imported_orphans: usize,
    pub unresolved_pending: usize,
    pub missing_storage_objects: usize,
    pub generated_thumbnails: usize,
    pub thumbnail_failures: usize,
}

pub async fn finalize_upload(
    store: &PhotoStore,
    catalog: &PhotoCatalog,
    id: &str,
) -> Result<(), FinalizeError> {
    store
        .storage_key(id)
        .map_err(FinalizeError::InvalidCaptureId)?;
    let expected = catalog
        .expected_size(id)
        .await
        .map_err(FinalizeError::Catalog)?
        .ok_or(FinalizeError::ReservationNotFound)?;
    let actual = store
        .uploaded_size(id)
        .await
        .map_err(FinalizeError::Storage)?
        .ok_or(FinalizeError::ObjectNotFound)?;
    if actual != expected {
        return Err(FinalizeError::SizeMismatch { expected, actual });
    }
    store
        .ensure_thumbnail(id)
        .await
        .map_err(FinalizeError::Storage)?;
    catalog.mark_ready(id).await.map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            FinalizeError::ReservationNotFound
        } else {
            FinalizeError::Catalog(error)
        }
    })?;
    catalog
        .mark_thumbnail_ready(id)
        .await
        .map_err(FinalizeError::Catalog)
}

pub async fn gallery_photos(catalog: &PhotoCatalog) -> io::Result<Vec<Photo>> {
    catalog.list().await
}

pub async fn reconcile_all(
    store: &PhotoStore,
    catalog: &PhotoCatalog,
) -> io::Result<ReconcileReport> {
    let ready_before = catalog.list().await?;
    let ready_before_ids = ready_before
        .iter()
        .map(|photo| photo.id.as_str())
        .collect::<HashSet<_>>();
    let pending = catalog
        .pending()
        .await?
        .into_iter()
        .map(|photo| (photo.id, photo.byte_size))
        .collect::<HashMap<_, _>>();
    let stored = store.list().await?;
    let stored_ids = stored
        .iter()
        .map(|photo| photo.id.as_str())
        .collect::<HashSet<_>>();

    let mut recovered_pending = 0;
    let mut generated_thumbnails = 0;
    let mut thumbnail_failures = 0;
    let mut unresolved = HashSet::new();
    for (id, expected_size) in &pending {
        match store.uploaded_size(id).await? {
            Some(actual_size) if actual_size == *expected_size => {
                match store.ensure_thumbnail(id).await {
                    Ok(generated) => {
                        catalog.mark_ready(id).await?;
                        catalog.mark_thumbnail_ready(id).await?;
                        recovered_pending += 1;
                        generated_thumbnails += usize::from(generated);
                    }
                    Err(_) => {
                        thumbnail_failures += 1;
                        unresolved.insert(id.as_str());
                    }
                }
            }
            _ => {
                unresolved.insert(id.as_str());
            }
        }
    }

    let orphan_records = stored
        .iter()
        .filter(|photo| !ready_before_ids.contains(photo.id.as_str()))
        .filter(|photo| !pending.contains_key(&photo.id))
        .map(|photo| {
            let key = store.storage_key(&photo.id)?;
            Ok((photo.clone(), key))
        })
        .collect::<io::Result<Vec<_>>>()?;
    catalog.import(&orphan_records).await?;

    for id in catalog.thumbnails_pending().await? {
        if !stored_ids.contains(id.as_str()) {
            continue;
        }
        match store.ensure_thumbnail(&id).await {
            Ok(generated) => {
                catalog.mark_thumbnail_ready(&id).await?;
                generated_thumbnails += usize::from(generated);
            }
            Err(_) => thumbnail_failures += 1,
        }
    }

    let ready_after = catalog.list().await?;
    let missing_storage_objects = ready_after
        .iter()
        .filter(|photo| !stored_ids.contains(photo.id.as_str()))
        .count();
    Ok(ReconcileReport {
        storage_objects: stored.len(),
        ready_before: ready_before.len(),
        ready_after: ready_after.len(),
        recovered_pending,
        imported_orphans: orphan_records.len(),
        unresolved_pending: unresolved.len(),
        missing_storage_objects,
        generated_thumbnails,
        thumbnail_failures,
    })
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;

    use image::{Rgb, RgbImage};

    use super::{FinalizeError, ReconcileReport, finalize_upload, gallery_photos, reconcile_all};
    use crate::catalog::PhotoCatalog;
    use crate::photos::PhotoStore;
    use axum::http::StatusCode;

    struct Fixture {
        root: PathBuf,
        store: PhotoStore,
        catalog: PhotoCatalog,
    }

    impl Fixture {
        async fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "daily-mirror-upload-flow-{name}-{}",
                std::process::id()
            ));
            let _ = tokio::fs::remove_dir_all(&root).await;
            tokio::fs::create_dir_all(&root).await.unwrap();
            Self {
                store: PhotoStore::new(root.join("photos")),
                catalog: PhotoCatalog::local(
                    root.join("catalog.db").to_string_lossy().into_owned(),
                ),
                root,
            }
        }

        async fn cleanup(self) {
            drop(self.catalog);
            let _ = tokio::fs::remove_dir_all(self.root).await;
        }
    }

    fn jpeg(payload: &[u8]) -> Vec<u8> {
        let color = payload.first().copied().unwrap_or(0);
        let image = RgbImage::from_pixel(4, 3, Rgb([color, 96, 180]));
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image)
            .unwrap();
        bytes
    }

    #[test]
    fn completion_errors_map_to_stable_http_statuses() {
        let invalid =
            FinalizeError::InvalidCaptureId(io::Error::new(io::ErrorKind::InvalidInput, "invalid"));
        let storage = FinalizeError::Storage(io::Error::other("storage"));
        let catalog = FinalizeError::Catalog(io::Error::other("catalog"));
        assert_eq!(invalid.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(
            FinalizeError::ReservationNotFound.status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            FinalizeError::ObjectNotFound.status_code(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            FinalizeError::SizeMismatch {
                expected: 5,
                actual: 4,
            }
            .status_code(),
            StatusCode::CONFLICT
        );
        assert_eq!(storage.status_code(), StatusCode::BAD_GATEWAY);
        assert_eq!(catalog.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn completion_requires_reservation_object_and_exact_size_and_is_idempotent() {
        let fixture = Fixture::new("completion").await;
        let id = "20260830T210000Z-complete1";
        let bytes = jpeg(&[1, 2, 3]);

        assert!(matches!(
            finalize_upload(&fixture.store, &fixture.catalog, id).await,
            Err(FinalizeError::ReservationNotFound)
        ));
        fixture
            .catalog
            .reserve(
                id,
                &fixture.store.storage_key(id).unwrap(),
                bytes.len() as u64,
            )
            .await
            .unwrap();
        assert!(matches!(
            finalize_upload(&fixture.store, &fixture.catalog, id).await,
            Err(FinalizeError::ObjectNotFound)
        ));

        fixture.store.save(id, &bytes).await.unwrap();
        finalize_upload(&fixture.store, &fixture.catalog, id)
            .await
            .unwrap();
        finalize_upload(&fixture.store, &fixture.catalog, id)
            .await
            .unwrap();
        let photo = &fixture.catalog.list().await.unwrap()[0];
        assert_eq!(photo.id, id);
        assert!(photo.thumbnail_url.is_some());
        assert!(fixture.store.read_thumbnail(id).await.unwrap().is_some());
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn completion_rejects_size_mismatch_and_leaves_row_pending() {
        let fixture = Fixture::new("mismatch").await;
        let id = "20260830T210100Z-mismatch1";
        let bytes = jpeg(&[9]);
        fixture
            .catalog
            .reserve(
                id,
                &fixture.store.storage_key(id).unwrap(),
                bytes.len() as u64 + 1,
            )
            .await
            .unwrap();
        fixture.store.save(id, &bytes).await.unwrap();

        assert!(matches!(
            finalize_upload(&fixture.store, &fixture.catalog, id).await,
            Err(FinalizeError::SizeMismatch { .. })
        ));
        assert!(fixture.catalog.list().await.unwrap().is_empty());
        assert_eq!(fixture.catalog.pending().await.unwrap()[0].id, id);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn gallery_reads_do_not_reconcile_complete_pending_uploads() {
        let fixture = Fixture::new("gallery-recovery").await;
        let id = "20260830T210200Z-recover01";
        let bytes = jpeg(&[4, 5]);
        fixture
            .catalog
            .reserve(
                id,
                &fixture.store.storage_key(id).unwrap(),
                bytes.len() as u64,
            )
            .await
            .unwrap();
        fixture.store.save(id, &bytes).await.unwrap();

        let photos = gallery_photos(&fixture.catalog).await.unwrap();
        assert!(photos.is_empty());
        assert_eq!(fixture.catalog.pending().await.unwrap()[0].id, id);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn gallery_reads_do_not_import_storage_orphans() {
        let fixture = Fixture::new("gallery-orphan").await;
        let pending_id = "20260830T210300Z-mismatch2";
        let orphan_id = "20260830T210301Z-orphan001";
        let bytes = jpeg(&[6]);
        fixture
            .catalog
            .reserve(
                pending_id,
                &fixture.store.storage_key(pending_id).unwrap(),
                bytes.len() as u64 + 1,
            )
            .await
            .unwrap();
        fixture.store.save(pending_id, &bytes).await.unwrap();
        fixture.store.save(orphan_id, &jpeg(&[7])).await.unwrap();

        let photos = gallery_photos(&fixture.catalog).await.unwrap();
        assert!(photos.is_empty());
        assert_eq!(fixture.catalog.pending().await.unwrap()[0].id, pending_id);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn daily_reconciliation_repairs_orphans_and_pending_rows_idempotently() {
        let fixture = Fixture::new("daily").await;
        let orphan_id = "20260830T210400Z-orphan002";
        let pending_id = "20260830T210401Z-recover02";
        let mismatch_id = "20260830T210402Z-mismatch3";
        let bytes = jpeg(&[8, 9]);
        fixture.store.save(orphan_id, &jpeg(&[7])).await.unwrap();
        for id in [pending_id, mismatch_id] {
            fixture.store.save(id, &bytes).await.unwrap();
        }
        fixture
            .catalog
            .reserve(
                pending_id,
                &fixture.store.storage_key(pending_id).unwrap(),
                bytes.len() as u64,
            )
            .await
            .unwrap();
        fixture
            .catalog
            .reserve(
                mismatch_id,
                &fixture.store.storage_key(mismatch_id).unwrap(),
                bytes.len() as u64 + 1,
            )
            .await
            .unwrap();

        let first = reconcile_all(&fixture.store, &fixture.catalog)
            .await
            .unwrap();
        assert_eq!(
            first,
            ReconcileReport {
                storage_objects: 3,
                ready_before: 0,
                ready_after: 2,
                recovered_pending: 1,
                imported_orphans: 1,
                unresolved_pending: 1,
                missing_storage_objects: 0,
                generated_thumbnails: 2,
                thumbnail_failures: 0,
            }
        );
        let second = reconcile_all(&fixture.store, &fixture.catalog)
            .await
            .unwrap();
        assert_eq!(second.ready_before, 2);
        assert_eq!(second.ready_after, 2);
        assert_eq!(second.recovered_pending, 0);
        assert_eq!(second.imported_orphans, 0);
        assert_eq!(second.unresolved_pending, 1);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn daily_reconciliation_reports_catalog_rows_missing_from_storage() {
        let fixture = Fixture::new("missing-storage").await;
        let id = "20260830T210500Z-missing01";
        fixture
            .catalog
            .register_ready(id, &fixture.store.storage_key(id).unwrap(), 123)
            .await
            .unwrap();

        let report = reconcile_all(&fixture.store, &fixture.catalog)
            .await
            .unwrap();
        assert_eq!(report.ready_after, 1);
        assert_eq!(report.missing_storage_objects, 1);
        assert_eq!(report.generated_thumbnails, 0);
        assert_eq!(report.thumbnail_failures, 0);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn thumbnail_reconciliation_reports_corrupt_images_without_hiding_them() {
        let fixture = Fixture::new("corrupt-thumbnail").await;
        let id = "20260830T210600Z-corrupt01";
        let corrupt = vec![0xff, 0xd8, 7, 0xff, 0xd9];
        fixture.store.save(id, &corrupt).await.unwrap();
        fixture
            .catalog
            .register_ready(
                id,
                &fixture.store.storage_key(id).unwrap(),
                corrupt.len() as u64,
            )
            .await
            .unwrap();

        let report = reconcile_all(&fixture.store, &fixture.catalog)
            .await
            .unwrap();
        assert_eq!(report.ready_after, 1);
        assert_eq!(report.generated_thumbnails, 0);
        assert_eq!(report.thumbnail_failures, 1);
        assert_eq!(fixture.catalog.list().await.unwrap()[0].thumbnail_url, None);
        fixture.cleanup().await;
    }
}
