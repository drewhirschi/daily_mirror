use std::io;
use std::sync::Arc;
use std::time::Duration;

use libsql::{Builder, Database, params};
use tokio::sync::OnceCell;

use crate::capture::{CaptureMetadata, PhotoCapture};
use crate::photos::Photo;

/// How long a local-file writer waits for another process's write lock.
const LOCAL_BUSY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct PhotoCatalog {
    inner: Arc<CatalogInner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPhoto {
    pub id: String,
    pub byte_size: u64,
}

#[derive(Debug)]
struct CatalogInner {
    location: CatalogLocation,
    database: OnceCell<Database>,
    /// Migrations (local) or the schema-version check (remote), run once.
    prepared: OnceCell<()>,
}

#[derive(Debug)]
enum CatalogLocation {
    Local(String),
    Remote { url: String, token: String },
}

impl PhotoCatalog {
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(CatalogInner {
                location: CatalogLocation::Local(path.into()),
                database: OnceCell::new(),
                prepared: OnceCell::new(),
            }),
        }
    }

    pub fn from_env() -> io::Result<Self> {
        let location = match std::env::var("DAILY_MIRROR_DATABASE_URL") {
            Ok(url) if url.starts_with("libsql://") || url.starts_with("https://") => {
                let token = std::env::var("DAILY_MIRROR_DATABASE_AUTH_TOKEN").map_err(|_| {
                    invalid_config("DAILY_MIRROR_DATABASE_AUTH_TOKEN is required for Turso")
                })?;
                CatalogLocation::Remote { url, token }
            }
            Ok(path) if !path.is_empty() => CatalogLocation::Local(path),
            _ => CatalogLocation::Local(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("data/daily-mirror.db")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        Ok(match location {
            CatalogLocation::Local(path) => Self::local(path),
            location @ CatalogLocation::Remote { .. } => Self {
                inner: Arc::new(CatalogInner {
                    location,
                    database: OnceCell::new(),
                    prepared: OnceCell::new(),
                }),
            },
        })
    }

    /// Open the database without touching its schema, so the health check can
    /// read the migration state of a database this build refuses to serve.
    async fn open(&self) -> io::Result<&Database> {
        self.inner
            .database
            .get_or_try_init(|| async {
                let database = match &self.inner.location {
                    CatalogLocation::Local(path) => {
                        if let Some(parent) = std::path::Path::new(path).parent() {
                            tokio::fs::create_dir_all(parent).await?;
                        }
                        Builder::new_local(path)
                            .build()
                            .await
                            .map_err(io::Error::other)?
                    }
                    CatalogLocation::Remote { url, token } => {
                        Builder::new_remote(url.clone(), token.clone())
                            .build()
                            .await
                            .map_err(io::Error::other)?
                    }
                };
                if matches!(&self.inner.location, CatalogLocation::Local(_)) {
                    let connection = database.connect().map_err(io::Error::other)?;
                    connection
                        .busy_timeout(LOCAL_BUSY_TIMEOUT)
                        .map_err(io::Error::other)?;
                    connection
                        .execute_batch("PRAGMA journal_mode = WAL;")
                        .await
                        .map_err(io::Error::other)?;
                }
                Ok(database)
            })
            .await
    }

    /// A connection that has not been schema-checked. Only the health check
    /// and the migration runner may use one.
    pub(crate) async fn raw_connection(&self) -> io::Result<libsql::Connection> {
        self.open().await?.connect().map_err(io::Error::other)
    }

    async fn database(&self) -> io::Result<&Database> {
        let database = self.open().await?;
        self.inner
            .prepared
            .get_or_try_init(|| async {
                // The schema itself lives in `server/migrations/`. A local file
                // is this process's own database, so it is migrated here; the
                // shared Turso database is only checked, because changing it
                // belongs to `daily-mirror-migrate up` before the deploy.
                crate::migrations::prepare(
                    &database.connect().map_err(io::Error::other)?,
                    matches!(&self.inner.location, CatalogLocation::Local(_)),
                )
                .await
            })
            .await?;
        Ok(database)
    }

    pub(crate) async fn connection(&self) -> io::Result<libsql::Connection> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        if matches!(self.inner.location, CatalogLocation::Local(_)) {
            // A local file is shared by every process of a split deployment (and by
            // concurrent hosted workers in the packaged tests), so a writer must wait
            // for the current writer instead of failing the request with SQLITE_BUSY.
            connection
                .busy_timeout(LOCAL_BUSY_TIMEOUT)
                .map_err(io::Error::other)?;
        }
        Ok(connection)
    }

    pub async fn reserve(&self, id: &str, storage_key: &str, byte_size: u64) -> io::Result<()> {
        self.reserve_for_device(
            id,
            storage_key,
            byte_size,
            None,
            &CaptureMetadata::default(),
        )
        .await
    }

    /// Reserve an upload slot, recording the device that captured it when the
    /// uploader authenticated with a per-device token, along with whatever the
    /// camera reported about the capture itself.
    pub async fn reserve_for_device(
        &self,
        id: &str,
        storage_key: &str,
        byte_size: u64,
        device_id: Option<&str>,
        capture: &CaptureMetadata,
    ) -> io::Result<()> {
        let connection = self.connection().await?;
        guard_completed_row(&connection, id, byte_size, device_id).await?;
        connection
            .execute(
                "INSERT INTO photos (id, storage_key, captured_at, byte_size, status, device_id,
                    firmware_version, sensor, width, height, jpeg_quality, exposure_us,
                    analog_gain, digital_gain, af_state, lens_position, colour_temperature_k,
                    mean_luma, focus_score, trigger, capture_source)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5,
                     ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
             ON CONFLICT(id) DO UPDATE SET
                byte_size = excluded.byte_size,
                captured_at = excluded.captured_at,
                device_id = COALESCE(excluded.device_id, photos.device_id),
                firmware_version = COALESCE(excluded.firmware_version, photos.firmware_version),
                sensor = COALESCE(excluded.sensor, photos.sensor),
                width = COALESCE(excluded.width, photos.width),
                height = COALESCE(excluded.height, photos.height),
                jpeg_quality = COALESCE(excluded.jpeg_quality, photos.jpeg_quality),
                exposure_us = COALESCE(excluded.exposure_us, photos.exposure_us),
                analog_gain = COALESCE(excluded.analog_gain, photos.analog_gain),
                digital_gain = COALESCE(excluded.digital_gain, photos.digital_gain),
                af_state = COALESCE(excluded.af_state, photos.af_state),
                lens_position = COALESCE(excluded.lens_position, photos.lens_position),
                colour_temperature_k =
                    COALESCE(excluded.colour_temperature_k, photos.colour_temperature_k),
                mean_luma = COALESCE(excluded.mean_luma, photos.mean_luma),
                focus_score = COALESCE(excluded.focus_score, photos.focus_score),
                trigger = COALESCE(excluded.trigger, photos.trigger),
                capture_source = COALESCE(excluded.capture_source, photos.capture_source),
                updated_at = CURRENT_TIMESTAMP",
                capture_params(id, storage_key, byte_size, device_id, capture),
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    /// The device that uploaded a photo, when one is recorded.
    pub async fn device_for_photo(&self, id: &str) -> io::Result<Option<String>> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query("SELECT device_id FROM photos WHERE id = ?1", params![id])
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        row.get::<Option<String>>(0).map_err(io::Error::other)
    }

    /// Reserve an onboarding capture so completion can attach its single face
    /// to the person being enrolled.
    pub async fn reserve_enrollment(
        &self,
        id: &str,
        storage_key: &str,
        byte_size: u64,
        person_id: &str,
        capture: &CaptureMetadata,
    ) -> io::Result<()> {
        self.reserve_for_device(id, storage_key, byte_size, None, capture)
            .await?;
        let connection = self.connection().await?;
        connection
            .execute(
                "UPDATE photos SET source = 'enrollment', enrollment_person_id = ?2,
                    updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![id, person_id],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn mark_ready(&self, id: &str) -> io::Result<()> {
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE photos SET status = 'ready', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id],
            )
            .await
            .map_err(io::Error::other)?;
        if changed == 0 {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no upload reservation exists for {id}"),
            ))
        } else {
            Ok(())
        }
    }

    pub async fn expected_size(&self, id: &str) -> io::Result<Option<u64>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query("SELECT byte_size FROM photos WHERE id = ?1", params![id])
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        let byte_size: i64 = row.get(0).map_err(io::Error::other)?;
        Ok(Some(positive_size(byte_size)?))
    }

    pub async fn pending(&self) -> io::Result<Vec<PendingPhoto>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, byte_size FROM photos WHERE status = 'pending' ORDER BY captured_at",
                (),
            )
            .await
            .map_err(io::Error::other)?;
        let mut pending = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            let byte_size: i64 = row.get(1).map_err(io::Error::other)?;
            pending.push(PendingPhoto {
                id,
                byte_size: positive_size(byte_size)?,
            });
        }
        Ok(pending)
    }

    pub async fn list(&self) -> io::Result<Vec<Photo>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                &format!(
                    "SELECT photos.id, photos.thumbnail_status, photos.media_revision,
                            photos.flipbook_excluded, {CAPTURE_COLUMNS}
                     FROM photos
                     LEFT JOIN devices ON devices.device_id = photos.device_id
                     WHERE photos.status = 'ready'
                     ORDER BY photos.captured_at DESC, photos.id DESC"
                ),
                (),
            )
            .await
            .map_err(io::Error::other)?;
        let mut photos = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            let thumbnail_status: String = row.get(1).map_err(io::Error::other)?;
            let media_revision: i64 = row.get(2).map_err(io::Error::other)?;
            let revision = u64::try_from(media_revision).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid photo media revision")
            })?;
            let flipbook_excluded: i64 = row.get(3).map_err(io::Error::other)?;
            let capture = read_capture(&row, 4)?;
            photos.push(Photo {
                url: format!("/api/photos/{id}?rev={revision}"),
                thumbnail_url: (thumbnail_status == "ready")
                    .then(|| format!("/api/photos/{id}/thumbnail?rev={revision}")),
                flipbook_excluded: flipbook_excluded != 0,
                capture: (!capture.is_empty()).then_some(capture),
                id,
            });
        }
        Ok(photos)
    }

    /// What is known about one photograph's capture, for the detail view.
    pub async fn capture_for_photo(&self, id: &str) -> io::Result<Option<PhotoCapture>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                &format!(
                    "SELECT {CAPTURE_COLUMNS} FROM photos
                     LEFT JOIN devices ON devices.device_id = photos.device_id
                     WHERE photos.id = ?1"
                ),
                params![id],
            )
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        Ok(Some(read_capture(&row, 0)?))
    }

    pub async fn ready_is_empty(&self) -> io::Result<bool> {
        let connection = self.connection().await?;
        let count = ready_photo_count(&connection).await?;
        Ok(count == 0)
    }

    pub async fn import(&self, photos: &[(Photo, String)]) -> io::Result<()> {
        let connection = self.connection().await?;
        for (photo, storage_key) in photos {
            connection
                .execute(
                    "INSERT INTO photos (id, storage_key, captured_at, status)
                 VALUES (?1, ?2, ?3, 'ready')
                 ON CONFLICT(id) DO UPDATE SET
                    storage_key = excluded.storage_key,
                    status = 'ready',
                    updated_at = CURRENT_TIMESTAMP",
                    params![
                        photo.id.clone(),
                        storage_key.clone(),
                        id_to_timestamp(&photo.id)
                    ],
                )
                .await
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub async fn register_ready(
        &self,
        id: &str,
        storage_key: &str,
        byte_size: u64,
    ) -> io::Result<()> {
        self.register_ready_for_device(
            id,
            storage_key,
            byte_size,
            None,
            &CaptureMetadata::default(),
        )
        .await
    }

    pub async fn register_ready_for_device(
        &self,
        id: &str,
        storage_key: &str,
        byte_size: u64,
        device_id: Option<&str>,
        capture: &CaptureMetadata,
    ) -> io::Result<()> {
        self.reserve_for_device(id, storage_key, byte_size, device_id, capture)
            .await?;
        self.mark_ready(id).await
    }

    pub async fn thumbnails_pending(&self) -> io::Result<Vec<String>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id FROM photos WHERE status = 'ready' AND thumbnail_status != 'ready' ORDER BY captured_at",
                (),
            )
            .await
            .map_err(io::Error::other)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            ids.push(row.get(0).map_err(io::Error::other)?);
        }
        Ok(ids)
    }

    pub async fn mark_thumbnail_ready(&self, id: &str) -> io::Result<()> {
        let connection = self.connection().await?;
        connection
            .execute(
                "UPDATE photos SET thumbnail_status = 'ready', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn record_rotation(&self, id: &str, degrees: i16, byte_size: u64) -> io::Result<()> {
        let connection = self.connection().await?;
        let byte_size = i64::try_from(byte_size)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "photo is too large"))?;
        let changed = connection.execute(
            "UPDATE photos SET rotation_degrees = (rotation_degrees + ?2 + 360) % 360, byte_size = ?3, media_revision = media_revision + 1, thumbnail_status = 'ready', updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND status = 'ready'",
            params![id, degrees, byte_size],
        ).await.map_err(io::Error::other)?;
        if changed == 0 {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no ready photo exists for {id}"),
            ))
        } else {
            Ok(())
        }
    }

    /// Returns `false` when no ready photo exists for `id`.
    pub async fn set_flipbook_excluded(&self, id: &str, excluded: bool) -> io::Result<bool> {
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE photos SET flipbook_excluded = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1 AND status = 'ready'",
                params![id, i64::from(excluded)],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(changed > 0)
    }

    pub async fn repair_ready_size(&self, id: &str, byte_size: u64) -> io::Result<()> {
        let connection = self.connection().await?;
        let byte_size = i64::try_from(byte_size)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "photo is too large"))?;
        let changed = connection
            .execute(
                "UPDATE photos SET byte_size = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1 AND status = 'ready'",
                params![id, byte_size],
            )
            .await
            .map_err(io::Error::other)?;
        if changed == 0 {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no ready photo exists for {id}"),
            ))
        } else {
            Ok(())
        }
    }

    pub async fn delete(&self, id: &str) -> io::Result<()> {
        let connection = self.connection().await?;
        connection
            .execute("DELETE FROM photos WHERE id = ?1", params![id])
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }
}

/// The provenance columns, in the order [`read_capture`] expects them.
const CAPTURE_COLUMNS: &str = "photos.device_id, devices.device_name, photos.firmware_version,
     photos.sensor, photos.width, photos.height, photos.jpeg_quality, photos.exposure_us,
     photos.analog_gain, photos.digital_gain, photos.af_state, photos.lens_position,
     photos.colour_temperature_k, photos.mean_luma, photos.focus_score, photos.trigger,
     photos.capture_source";

fn read_capture(row: &libsql::Row, offset: i32) -> io::Result<PhotoCapture> {
    let text = |index: i32| -> io::Result<Option<String>> {
        row.get::<Option<String>>(offset + index)
            .map_err(io::Error::other)
    };
    let number = |index: i32| -> io::Result<Option<i64>> {
        row.get::<Option<i64>>(offset + index)
            .map_err(io::Error::other)
    };
    let real = |index: i32| -> io::Result<Option<f64>> {
        row.get::<Option<f64>>(offset + index)
            .map_err(io::Error::other)
    };
    Ok(PhotoCapture {
        device_id: text(0)?,
        device_name: text(1)?,
        firmware_version: text(2)?,
        sensor: text(3)?,
        width: number(4)?,
        height: number(5)?,
        jpeg_quality: number(6)?,
        exposure_us: number(7)?,
        analog_gain: real(8)?,
        digital_gain: real(9)?,
        af_state: text(10)?,
        lens_position: real(11)?,
        colour_temperature_k: number(12)?,
        mean_luma: number(13)?,
        focus_score: number(14)?,
        trigger: text(15)?,
        capture_source: text(16)?,
    })
}

fn capture_params(
    id: &str,
    storage_key: &str,
    byte_size: u64,
    device_id: Option<&str>,
    capture: &CaptureMetadata,
) -> Vec<libsql::Value> {
    use libsql::Value;
    let text = |value: &Option<String>| value.clone().map_or(Value::Null, Value::Text);
    let number = |value: Option<i64>| value.map_or(Value::Null, Value::Integer);
    let real = |value: Option<f64>| value.map_or(Value::Null, Value::Real);
    vec![
        Value::Text(id.to_owned()),
        Value::Text(storage_key.to_owned()),
        Value::Text(
            capture
                .captured_at
                .clone()
                .unwrap_or_else(|| id_to_timestamp(id)),
        ),
        Value::Integer(byte_size as i64),
        device_id.map_or(Value::Null, |value| Value::Text(value.to_owned())),
        text(&capture.firmware_version),
        text(&capture.sensor),
        number(capture.width),
        number(capture.height),
        number(capture.jpeg_quality),
        number(capture.exposure_us),
        real(capture.analog_gain),
        real(capture.digital_gain),
        text(&capture.af_state),
        real(capture.lens_position),
        number(capture.colour_temperature_k),
        number(capture.mean_luma),
        number(capture.focus_score),
        text(&capture.trigger),
        text(&capture.capture_source),
    ]
}

/// Reservation upserts on the capture ID, which is fine for a camera retrying
/// its own upload and catastrophic for two cameras that pick the same ID: the
/// second grant would silently repoint a finished photograph at a different
/// object. A completed row is therefore only re-reserved when the request
/// describes the very same upload.
async fn guard_completed_row(
    connection: &libsql::Connection,
    id: &str,
    byte_size: u64,
    device_id: Option<&str>,
) -> io::Result<()> {
    let mut rows = connection
        .query(
            "SELECT status, byte_size, device_id FROM photos WHERE id = ?1",
            params![id],
        )
        .await
        .map_err(io::Error::other)?;
    let Some(row) = rows.next().await.map_err(io::Error::other)? else {
        return Ok(());
    };
    let status: String = row.get(0).map_err(io::Error::other)?;
    if status != "ready" {
        return Ok(());
    }
    let existing_size: Option<i64> = row.get(1).map_err(io::Error::other)?;
    let existing_device: Option<String> = row.get(2).map_err(io::Error::other)?;
    let same_size = existing_size == Some(byte_size as i64);
    // A request that names no device never claims one away from a row.
    let same_device = device_id.is_none() || device_id == existing_device.as_deref();
    if same_size && same_device {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("capture {id} is already stored; a different photograph may not reuse its ID"),
    ))
}

async fn ready_photo_count(connection: &libsql::Connection) -> io::Result<i64> {
    let mut rows = connection
        .query("SELECT COUNT(*) FROM photos WHERE status = 'ready'", ())
        .await
        .map_err(io::Error::other)?;
    let row = rows
        .next()
        .await
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::other("photo count query returned no row"))?;
    row.get(0).map_err(io::Error::other)
}

fn positive_size(byte_size: i64) -> io::Result<u64> {
    u64::try_from(byte_size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid photo byte size"))
}

fn id_to_timestamp(id: &str) -> String {
    if id.len() >= 16 {
        format!(
            "{}-{}-{}T{}:{}:{}Z",
            &id[0..4],
            &id[4..6],
            &id[6..8],
            &id[9..11],
            &id[11..13],
            &id[13..15]
        )
    } else {
        id.to_owned()
    }
}

fn invalid_config(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use crate::photos::Photo;

    use super::{PendingPhoto, PhotoCatalog};

    #[tokio::test]
    async fn local_catalog_tracks_ready_rotation_and_deletion() {
        let path = std::env::temp_dir().join(format!(
            "daily-mirror-catalog-test-{}.db",
            std::process::id()
        ));
        let _ = tokio::fs::remove_file(&path).await;
        let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
        let id = "20260829T071500Z-catalog1";

        catalog.reserve(id, "photos/test.jpg", 1234).await.unwrap();
        assert!(catalog.list().await.unwrap().is_empty());
        assert!(catalog.ready_is_empty().await.unwrap());
        assert_eq!(
            catalog.pending().await.unwrap(),
            vec![PendingPhoto {
                id: id.to_owned(),
                byte_size: 1234,
            }]
        );
        assert_eq!(catalog.expected_size(id).await.unwrap(), Some(1234));
        catalog.mark_ready(id).await.unwrap();
        assert!(!catalog.ready_is_empty().await.unwrap());
        assert!(catalog.pending().await.unwrap().is_empty());
        assert_eq!(catalog.thumbnails_pending().await.unwrap(), vec![id]);
        let photo = &catalog.list().await.unwrap()[0];
        assert_eq!(photo.id, id);
        assert_eq!(photo.url, format!("/api/photos/{id}?rev=0"));
        assert_eq!(photo.thumbnail_url, None);
        catalog.mark_thumbnail_ready(id).await.unwrap();
        assert_eq!(
            catalog.list().await.unwrap()[0].thumbnail_url,
            Some(format!("/api/photos/{id}/thumbnail?rev=0"))
        );
        catalog.record_rotation(id, 90, 987).await.unwrap();
        assert_eq!(catalog.expected_size(id).await.unwrap(), Some(987));
        let rotated = &catalog.list().await.unwrap()[0];
        assert_eq!(rotated.url, format!("/api/photos/{id}?rev=1"));
        assert_eq!(
            rotated.thumbnail_url,
            Some(format!("/api/photos/{id}/thumbnail?rev=1"))
        );
        catalog.delete(id).await.unwrap();
        assert!(catalog.list().await.unwrap().is_empty());
        let missing = catalog.mark_ready(id).await.unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);

        catalog.reserve(id, "photos/test.jpg", 1234).await.unwrap();
        catalog
            .import(&[(
                Photo {
                    id: id.to_owned(),
                    url: format!("/api/photos/{id}"),
                    thumbnail_url: None,
                    flipbook_excluded: false,
                    capture: None,
                },
                "photos/test.jpg".to_owned(),
            )])
            .await
            .unwrap();
        assert_eq!(catalog.list().await.unwrap()[0].id, id);

        drop(catalog);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn existing_catalogs_gain_thumbnail_columns_without_losing_photos() {
        let path = std::env::temp_dir().join(format!(
            "daily-mirror-catalog-migration-test-{}.db",
            std::process::id()
        ));
        let _ = tokio::fs::remove_file(&path).await;
        let database = libsql::Builder::new_local(&path).build().await.unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE photos (
                    id TEXT PRIMARY KEY,
                    storage_key TEXT NOT NULL,
                    captured_at TEXT NOT NULL,
                    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
                    byte_size INTEGER,
                    status TEXT NOT NULL DEFAULT 'pending',
                    rotation_degrees INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO photos (id, storage_key, captured_at, byte_size, status)
                VALUES ('20260829T071500Z-migrate1', 'photos/migrate.jpg', '2026-08-29T07:15:00Z', 123, 'ready');",
            )
            .await
            .unwrap();
        drop(connection);
        drop(database);

        let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
        let photos = catalog.list().await.unwrap();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].url, "/api/photos/20260829T071500Z-migrate1?rev=0");
        assert_eq!(photos[0].thumbnail_url, None);
        assert_eq!(
            catalog.thumbnails_pending().await.unwrap(),
            vec!["20260829T071500Z-migrate1"]
        );

        drop(catalog);
        let _ = tokio::fs::remove_file(path).await;
    }
}
