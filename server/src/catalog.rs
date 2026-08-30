use std::io;
use std::sync::Arc;

use libsql::{Builder, Database, params};
use tokio::sync::OnceCell;

use crate::photos::Photo;

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
}

#[derive(Debug)]
enum CatalogLocation {
    Local(String),
    Remote { url: String, token: String },
}

impl PhotoCatalog {
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
        Ok(Self {
            inner: Arc::new(CatalogInner {
                location,
                database: OnceCell::new(),
            }),
        })
    }

    async fn database(&self) -> io::Result<&Database> {
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
                let connection = database.connect().map_err(io::Error::other)?;
                connection
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS photos (
                    id TEXT PRIMARY KEY,
                    storage_key TEXT NOT NULL,
                    captured_at TEXT NOT NULL,
                    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
                    byte_size INTEGER,
                    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'ready')),
                    rotation_degrees INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS photos_ready_captured_at
                    ON photos(status, captured_at DESC);",
                    )
                    .await
                    .map_err(io::Error::other)?;
                Ok(database)
            })
            .await
    }

    pub async fn reserve(&self, id: &str, storage_key: &str, byte_size: u64) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection.execute(
            "INSERT INTO photos (id, storage_key, captured_at, byte_size, status)
             VALUES (?1, ?2, ?3, ?4, 'pending')
             ON CONFLICT(id) DO UPDATE SET byte_size = excluded.byte_size, updated_at = CURRENT_TIMESTAMP",
            params![id, storage_key, id_to_timestamp(id), byte_size as i64],
        ).await.map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn mark_ready(&self, id: &str) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
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
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
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
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
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
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT id FROM photos WHERE status = 'ready' ORDER BY captured_at DESC, id DESC",
                (),
            )
            .await
            .map_err(io::Error::other)?;
        let mut photos = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            photos.push(Photo {
                url: format!("/api/photos/{id}"),
                id,
            });
        }
        Ok(photos)
    }

    pub async fn ready_is_empty(&self) -> io::Result<bool> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let count = ready_photo_count(&connection).await?;
        Ok(count == 0)
    }

    pub async fn import(&self, photos: &[(Photo, String)]) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
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
        self.reserve(id, storage_key, byte_size).await?;
        self.mark_ready(id).await
    }

    pub async fn record_rotation(&self, id: &str, degrees: i16) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection.execute(
            "UPDATE photos SET rotation_degrees = (rotation_degrees + ?2 + 360) % 360, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, degrees],
        ).await.map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute("DELETE FROM photos WHERE id = ?1", params![id])
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }
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
    use std::sync::Arc;

    use tokio::sync::OnceCell;

    use crate::photos::Photo;

    use super::{CatalogInner, CatalogLocation, PendingPhoto, PhotoCatalog};

    #[tokio::test]
    async fn local_catalog_tracks_ready_rotation_and_deletion() {
        let path = std::env::temp_dir().join(format!(
            "daily-mirror-catalog-test-{}.db",
            std::process::id()
        ));
        let _ = tokio::fs::remove_file(&path).await;
        let catalog = PhotoCatalog {
            inner: Arc::new(CatalogInner {
                location: CatalogLocation::Local(path.to_string_lossy().into_owned()),
                database: OnceCell::new(),
            }),
        };
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
        assert_eq!(catalog.list().await.unwrap()[0].id, id);
        catalog.record_rotation(id, 90).await.unwrap();
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
                },
                "photos/test.jpg".to_owned(),
            )])
            .await
            .unwrap();
        assert_eq!(catalog.list().await.unwrap()[0].id, id);

        drop(catalog);
        let _ = tokio::fs::remove_file(path).await;
    }
}
