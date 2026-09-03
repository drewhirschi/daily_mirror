use std::io;
use std::sync::Arc;

use axum::http::StatusCode;
use daily_mirror_vision_contract::{
    ClaimRequest, ClaimedPhoto, DEFAULT_LEASE_SECONDS, DEFAULT_PIPELINE_VERSION, FaceResult,
    MAX_CLAIM_LIMIT, PhotoAnalysisResult, QueueStatus,
};
use libsql::{TransactionBehavior, params};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::catalog::PhotoCatalog;

const MAX_FACES_PER_PHOTO: usize = 100;
const MAX_LANDMARKS_PER_FACE: usize = 1_000;
const MAX_EMBEDDING_DIMENSION: usize = 4_096;

#[derive(Clone, Debug)]
pub struct ProcessingQueue {
    pub(crate) catalog: PhotoCatalog,
    schema: Arc<OnceCell<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteOutcome {
    Completed,
    AlreadyCompleted,
}

#[derive(Debug)]
pub enum ProcessingError {
    InvalidInput(String),
    NotFound,
    LeaseLost,
    Storage(io::Error),
}

impl ProcessingError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidInput(_) => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::LeaseLost => StatusCode::CONFLICT,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl ProcessingQueue {
    pub fn new(catalog: PhotoCatalog) -> Self {
        Self {
            catalog,
            schema: Arc::new(OnceCell::new()),
        }
    }

    pub async fn enqueue_active_photo(&self, photo_id: &str) -> io::Result<bool> {
        let pipeline_version = active_pipeline_version()?;
        self.enqueue_photo(photo_id, &pipeline_version).await
    }

    pub async fn enqueue_photo(&self, photo_id: &str, pipeline_version: &str) -> io::Result<bool> {
        validate_identifier("pipeline version", pipeline_version)?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let changed = connection
            .execute(
                "INSERT INTO photo_processing (photo_id, pipeline_version, status)
                 SELECT id, ?2, 'pending' FROM photos WHERE id = ?1 AND status = 'ready'
                 ON CONFLICT(photo_id, pipeline_version) DO NOTHING",
                params![photo_id, pipeline_version],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(changed > 0)
    }

    pub async fn reconcile_missing(&self, pipeline_version: &str) -> io::Result<u64> {
        validate_identifier("pipeline version", pipeline_version)?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        connection
            .execute(
                "INSERT INTO photo_processing (photo_id, pipeline_version, status)
                 SELECT id, ?1, 'pending' FROM photos WHERE status = 'ready'
                 ON CONFLICT(photo_id, pipeline_version) DO NOTHING",
                params![pipeline_version],
            )
            .await
            .map_err(io::Error::other)
    }

    pub async fn status(&self, pipeline_version: &str) -> io::Result<QueueStatus> {
        validate_identifier("pipeline version", pipeline_version)?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT
                    COALESCE(SUM(CASE WHEN status = 'pending' OR (status = 'leased' AND lease_expires_at <= CURRENT_TIMESTAMP) THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'leased' AND lease_expires_at > CURRENT_TIMESTAMP THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'complete' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM photo_processing WHERE pipeline_version = ?1",
                params![pipeline_version],
            )
            .await
            .map_err(io::Error::other)?;
        let row = rows
            .next()
            .await
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("processing status query returned no row"))?;
        Ok(QueueStatus {
            pipeline_version: pipeline_version.to_owned(),
            pending: nonnegative_count(row.get(0).map_err(io::Error::other)?)?,
            leased: nonnegative_count(row.get(1).map_err(io::Error::other)?)?,
            complete: nonnegative_count(row.get(2).map_err(io::Error::other)?)?,
            failed: nonnegative_count(row.get(3).map_err(io::Error::other)?)?,
        })
    }

    pub async fn claim(
        &self,
        request: &ClaimRequest,
    ) -> Result<Vec<ClaimedPhoto>, ProcessingError> {
        validate_identifier("worker ID", &request.worker_id)
            .map_err(|error| ProcessingError::InvalidInput(error.to_string()))?;
        validate_identifier("pipeline version", &request.pipeline_version)
            .map_err(|error| ProcessingError::InvalidInput(error.to_string()))?;
        if request.limit == 0 || request.limit > MAX_CLAIM_LIMIT {
            return Err(ProcessingError::InvalidInput(format!(
                "claim limit must be between 1 and {MAX_CLAIM_LIMIT}"
            )));
        }

        self.ensure_schema()
            .await
            .map_err(ProcessingError::Storage)?;
        let connection = self
            .catalog
            .connection()
            .await
            .map_err(ProcessingError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        let mut rows = transaction
            .query(
                "SELECT processing.photo_id, photos.byte_size
                 FROM photo_processing AS processing
                 JOIN photos ON photos.id = processing.photo_id
                 WHERE processing.pipeline_version = ?1
                   AND photos.status = 'ready'
                   AND (
                     (processing.status = 'pending' AND processing.available_at <= CURRENT_TIMESTAMP)
                     OR (processing.status = 'leased' AND processing.lease_expires_at <= CURRENT_TIMESTAMP)
                   )
                 ORDER BY photos.captured_at, photos.id
                 LIMIT ?2",
                params![request.pipeline_version.clone(), i64::from(request.limit)],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        let mut candidates = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?
        {
            let photo_id: String = row
                .get(0)
                .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
            let byte_size = row
                .get::<Option<i64>>(1)
                .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
            let expected_bytes = byte_size
                .map(nonnegative_size)
                .transpose()
                .map_err(ProcessingError::Storage)?;
            candidates.push((photo_id, expected_bytes));
        }
        drop(rows);

        let mut claimed = Vec::with_capacity(candidates.len());
        for (photo_id, expected_bytes) in candidates {
            let lease_token = Uuid::new_v4().to_string();
            let changed = transaction
                .execute(
                    "UPDATE photo_processing
                     SET status = 'leased', lease_token = ?3, leased_by = ?4,
                         lease_expires_at = datetime(CURRENT_TIMESTAMP, '+' || ?5 || ' seconds'),
                         attempt_count = attempt_count + 1, last_error = NULL,
                         updated_at = CURRENT_TIMESTAMP
                     WHERE photo_id = ?1 AND pipeline_version = ?2
                       AND (
                         (status = 'pending' AND available_at <= CURRENT_TIMESTAMP)
                         OR (status = 'leased' AND lease_expires_at <= CURRENT_TIMESTAMP)
                       )",
                    params![
                        photo_id.clone(),
                        request.pipeline_version.clone(),
                        lease_token.clone(),
                        request.worker_id.clone(),
                        DEFAULT_LEASE_SECONDS as i64,
                    ],
                )
                .await
                .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
            if changed == 1 {
                claimed.push(ClaimedPhoto {
                    download_url: format!("/api/processing/photos/{photo_id}"),
                    photo_id,
                    lease_token,
                    expected_bytes,
                    lease_seconds: DEFAULT_LEASE_SECONDS,
                });
            }
        }
        transaction
            .commit()
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        Ok(claimed)
    }

    pub async fn renew(
        &self,
        photo_id: &str,
        pipeline_version: &str,
        lease_token: &str,
    ) -> Result<(), ProcessingError> {
        validate_lease_input(pipeline_version, lease_token)?;
        self.ensure_schema()
            .await
            .map_err(ProcessingError::Storage)?;
        let connection = self
            .catalog
            .connection()
            .await
            .map_err(ProcessingError::Storage)?;
        let changed = connection
            .execute(
                "UPDATE photo_processing
                 SET lease_expires_at = datetime(CURRENT_TIMESTAMP, '+' || ?4 || ' seconds'),
                     updated_at = CURRENT_TIMESTAMP
                 WHERE photo_id = ?1 AND pipeline_version = ?2 AND status = 'leased'
                   AND lease_token = ?3 AND lease_expires_at > CURRENT_TIMESTAMP",
                params![
                    photo_id,
                    pipeline_version,
                    lease_token,
                    DEFAULT_LEASE_SECONDS as i64,
                ],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ProcessingError::LeaseLost)
        }
    }

    pub async fn complete(
        &self,
        photo_id: &str,
        pipeline_version: &str,
        lease_token: &str,
        result: &PhotoAnalysisResult,
    ) -> Result<CompleteOutcome, ProcessingError> {
        validate_lease_input(pipeline_version, lease_token)?;
        validate_result(result)?;
        self.ensure_schema()
            .await
            .map_err(ProcessingError::Storage)?;
        let connection = self
            .catalog
            .connection()
            .await
            .map_err(ProcessingError::Storage)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;

        let mut rows = transaction
            .query(
                "SELECT status, lease_token FROM photo_processing
                 WHERE photo_id = ?1 AND pipeline_version = ?2",
                params![photo_id, pipeline_version],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?
        else {
            return Err(ProcessingError::NotFound);
        };
        let status: String = row
            .get(0)
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        let current_token: Option<String> = row
            .get(1)
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        drop(rows);
        if status == "complete" && current_token.as_deref() == Some(lease_token) {
            return Ok(CompleteOutcome::AlreadyCompleted);
        }

        let valid_lease = transaction
            .execute(
                "UPDATE photo_processing SET updated_at = CURRENT_TIMESTAMP
                 WHERE photo_id = ?1 AND pipeline_version = ?2 AND status = 'leased'
                   AND lease_token = ?3 AND lease_expires_at > CURRENT_TIMESTAMP",
                params![photo_id, pipeline_version, lease_token],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        if valid_lease != 1 {
            return Err(ProcessingError::LeaseLost);
        }

        transaction
            .execute(
                "DELETE FROM faces WHERE photo_id = ?1 AND pipeline_version = ?2",
                params![photo_id, pipeline_version],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        transaction
            .execute(
                "DELETE FROM photo_analyses WHERE photo_id = ?1 AND pipeline_version = ?2",
                params![photo_id, pipeline_version],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        transaction
            .execute(
                "INSERT INTO photo_analyses (
                    photo_id, pipeline_version, oriented_width, oriented_height,
                    original_sha256, face_count, processing_millis
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    photo_id,
                    pipeline_version,
                    i64::from(result.oriented_width),
                    i64::from(result.oriented_height),
                    result.original_sha256.clone(),
                    result.faces.len() as i64,
                    checked_i64(result.processing_millis, "processing duration")
                        .map_err(ProcessingError::InvalidInput)?,
                ],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;

        for (ordinal, face) in result.faces.iter().enumerate() {
            insert_face(&transaction, photo_id, pipeline_version, ordinal, face).await?;
        }

        transaction
            .execute(
                "UPDATE photo_processing
                 SET status = 'complete', completed_at = CURRENT_TIMESTAMP,
                     leased_by = NULL, lease_expires_at = NULL,
                     updated_at = CURRENT_TIMESTAMP, last_error = NULL
                 WHERE photo_id = ?1 AND pipeline_version = ?2 AND lease_token = ?3",
                params![photo_id, pipeline_version, lease_token],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        transaction
            .commit()
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        Ok(CompleteOutcome::Completed)
    }

    pub async fn fail(
        &self,
        photo_id: &str,
        pipeline_version: &str,
        lease_token: &str,
        retryable: bool,
        error: &str,
    ) -> Result<(), ProcessingError> {
        validate_lease_input(pipeline_version, lease_token)?;
        let error = error.trim();
        if error.is_empty() || error.len() > 500 {
            return Err(ProcessingError::InvalidInput(
                "error must contain between 1 and 500 bytes".to_owned(),
            ));
        }
        self.ensure_schema()
            .await
            .map_err(ProcessingError::Storage)?;
        let connection = self
            .catalog
            .connection()
            .await
            .map_err(ProcessingError::Storage)?;
        let next_status = if retryable { "pending" } else { "failed" };
        let changed = connection
            .execute(
                "UPDATE photo_processing
                 SET status = ?4,
                     available_at = CASE WHEN ?4 = 'pending'
                         THEN datetime(CURRENT_TIMESTAMP, '+30 seconds')
                         ELSE available_at END,
                     lease_token = NULL, leased_by = NULL, lease_expires_at = NULL,
                     last_error = ?5, updated_at = CURRENT_TIMESTAMP
                 WHERE photo_id = ?1 AND pipeline_version = ?2 AND status = 'leased'
                   AND lease_token = ?3 AND lease_expires_at > CURRENT_TIMESTAMP",
                params![photo_id, pipeline_version, lease_token, next_status, error],
            )
            .await
            .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ProcessingError::LeaseLost)
        }
    }

    pub async fn reset_photo(&self, photo_id: &str) -> io::Result<()> {
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute("DELETE FROM faces WHERE photo_id = ?1", params![photo_id])
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "DELETE FROM photo_analyses WHERE photo_id = ?1",
                params![photo_id],
            )
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "UPDATE photo_processing
                 SET status = 'pending', available_at = CURRENT_TIMESTAMP,
                     lease_token = NULL, leased_by = NULL, lease_expires_at = NULL,
                     last_error = NULL, completed_at = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE photo_id = ?1",
                params![photo_id],
            )
            .await
            .map_err(io::Error::other)?;
        transaction.commit().await.map_err(io::Error::other)
    }

    pub async fn delete_photo(&self, photo_id: &str) -> io::Result<()> {
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute("DELETE FROM faces WHERE photo_id = ?1", params![photo_id])
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "DELETE FROM photo_analyses WHERE photo_id = ?1",
                params![photo_id],
            )
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "DELETE FROM photo_processing WHERE photo_id = ?1",
                params![photo_id],
            )
            .await
            .map_err(io::Error::other)?;
        transaction.commit().await.map_err(io::Error::other)
    }

    pub(crate) async fn ensure_schema(&self) -> io::Result<()> {
        self.schema
            .get_or_try_init(|| async {
                let connection = self.catalog.connection().await?;
                connection
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS photo_processing (
                            photo_id TEXT NOT NULL,
                            pipeline_version TEXT NOT NULL,
                            status TEXT NOT NULL DEFAULT 'pending'
                                CHECK(status IN ('pending', 'leased', 'complete', 'failed')),
                            available_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            lease_token TEXT,
                            leased_by TEXT,
                            lease_expires_at TEXT,
                            attempt_count INTEGER NOT NULL DEFAULT 0,
                            last_error TEXT,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            completed_at TEXT,
                            PRIMARY KEY(photo_id, pipeline_version)
                        );
                        CREATE INDEX IF NOT EXISTS photo_processing_claim
                            ON photo_processing(pipeline_version, status, available_at, lease_expires_at);
                        CREATE TABLE IF NOT EXISTS photo_analyses (
                            photo_id TEXT NOT NULL,
                            pipeline_version TEXT NOT NULL,
                            oriented_width INTEGER NOT NULL,
                            oriented_height INTEGER NOT NULL,
                            original_sha256 TEXT,
                            face_count INTEGER NOT NULL,
                            processing_millis INTEGER NOT NULL,
                            completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            PRIMARY KEY(photo_id, pipeline_version)
                        );
                        CREATE TABLE IF NOT EXISTS faces (
                            id TEXT PRIMARY KEY,
                            photo_id TEXT NOT NULL,
                            pipeline_version TEXT NOT NULL,
                            ordinal INTEGER NOT NULL,
                            detector_confidence REAL NOT NULL,
                            bounds_x REAL NOT NULL,
                            bounds_y REAL NOT NULL,
                            bounds_width REAL NOT NULL,
                            bounds_height REAL NOT NULL,
                            landmark_model TEXT NOT NULL,
                            landmark_schema TEXT NOT NULL,
                            landmarks_json TEXT NOT NULL,
                            embedding_model TEXT NOT NULL,
                            embedding BLOB NOT NULL,
                            embedding_dimension INTEGER NOT NULL,
                            person_id TEXT,
                            identity_state TEXT NOT NULL DEFAULT 'unknown'
                                CHECK(identity_state IN ('unknown', 'proposed', 'confirmed')),
                            identity_source TEXT,
                            identity_score REAL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            UNIQUE(photo_id, pipeline_version, ordinal)
                        );
                        CREATE INDEX IF NOT EXISTS faces_photo
                            ON faces(photo_id, pipeline_version, ordinal);
                        CREATE TABLE IF NOT EXISTS people (
                            id TEXT PRIMARY KEY,
                            display_name TEXT NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                        );
                        CREATE INDEX IF NOT EXISTS people_display_name
                            ON people(display_name COLLATE NOCASE);
                        CREATE INDEX IF NOT EXISTS faces_person
                            ON faces(person_id, pipeline_version, photo_id);
                        CREATE TABLE IF NOT EXISTS households (
                            id TEXT PRIMARY KEY,
                            display_name TEXT NOT NULL,
                            grid_size INTEGER NOT NULL DEFAULT 4
                                CHECK(grid_size IN (4, 6)),
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                        );
                        CREATE INDEX IF NOT EXISTS households_display_name
                            ON households(display_name COLLATE NOCASE);
                        CREATE TABLE IF NOT EXISTS household_members (
                            household_id TEXT NOT NULL,
                            person_id TEXT NOT NULL,
                            position INTEGER NOT NULL,
                            PRIMARY KEY(household_id, person_id),
                            UNIQUE(household_id, position)
                        );
                        CREATE INDEX IF NOT EXISTS household_members_person
                            ON household_members(person_id, household_id);",
                    )
                    .await
                    .map(|_| ())
                    .map_err(io::Error::other)
            })
            .await
            .copied()
    }
}

pub fn active_pipeline_version() -> io::Result<String> {
    let value = std::env::var("DAILY_MIRROR_PROCESSING_PIPELINE")
        .unwrap_or_else(|_| DEFAULT_PIPELINE_VERSION.to_owned());
    validate_identifier("active pipeline version", &value)?;
    Ok(value)
}

async fn insert_face(
    transaction: &libsql::Transaction,
    photo_id: &str,
    pipeline_version: &str,
    ordinal: usize,
    face: &FaceResult,
) -> Result<(), ProcessingError> {
    let landmarks = serde_json::to_string(&face.landmarks)
        .map_err(|error| ProcessingError::InvalidInput(error.to_string()))?;
    let embedding = embedding_bytes(&face.embedding);
    transaction
        .execute(
            "INSERT INTO faces (
                id, photo_id, pipeline_version, ordinal, detector_confidence,
                bounds_x, bounds_y, bounds_width, bounds_height,
                landmark_model, landmark_schema, landmarks_json,
                embedding_model, embedding, embedding_dimension
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                Uuid::new_v4().to_string(),
                photo_id,
                pipeline_version,
                ordinal as i64,
                f64::from(face.detector_confidence),
                f64::from(face.bounds.x),
                f64::from(face.bounds.y),
                f64::from(face.bounds.width),
                f64::from(face.bounds.height),
                face.landmark_model.clone(),
                face.landmark_schema.clone(),
                landmarks,
                face.embedding_model.clone(),
                embedding,
                face.embedding.len() as i64,
            ],
        )
        .await
        .map_err(|error| ProcessingError::Storage(io::Error::other(error)))?;
    Ok(())
}

fn validate_lease_input(pipeline_version: &str, lease_token: &str) -> Result<(), ProcessingError> {
    validate_identifier("pipeline version", pipeline_version)
        .map_err(|error| ProcessingError::InvalidInput(error.to_string()))?;
    Uuid::parse_str(lease_token)
        .map_err(|_| ProcessingError::InvalidInput("invalid lease token".to_owned()))?;
    Ok(())
}

fn validate_result(result: &PhotoAnalysisResult) -> Result<(), ProcessingError> {
    if result.oriented_width == 0 || result.oriented_height == 0 {
        return Err(ProcessingError::InvalidInput(
            "oriented image dimensions must be positive".to_owned(),
        ));
    }
    if result.faces.len() > MAX_FACES_PER_PHOTO {
        return Err(ProcessingError::InvalidInput(format!(
            "a photo may contain at most {MAX_FACES_PER_PHOTO} faces"
        )));
    }
    if let Some(digest) = &result.original_sha256
        && (digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(ProcessingError::InvalidInput(
            "original_sha256 must be 64 hexadecimal characters".to_owned(),
        ));
    }
    for face in &result.faces {
        validate_face(face)?;
    }
    Ok(())
}

fn validate_face(face: &FaceResult) -> Result<(), ProcessingError> {
    if !in_range(face.detector_confidence, 0.0, 1.0) {
        return Err(ProcessingError::InvalidInput(
            "detector confidence must be between zero and one".to_owned(),
        ));
    }
    let bounds = face.bounds;
    if !in_range(bounds.x, 0.0, 1.0)
        || !in_range(bounds.y, 0.0, 1.0)
        || !in_range(bounds.width, f32::EPSILON, 1.0)
        || !in_range(bounds.height, f32::EPSILON, 1.0)
        || bounds.x + bounds.width > 1.001
        || bounds.y + bounds.height > 1.001
    {
        return Err(ProcessingError::InvalidInput(
            "face bounds must be finite normalized coordinates".to_owned(),
        ));
    }
    validate_model_name("landmark model", &face.landmark_model)?;
    validate_model_name("landmark schema", &face.landmark_schema)?;
    validate_model_name("embedding model", &face.embedding_model)?;
    if face.landmarks.is_empty() || face.landmarks.len() > MAX_LANDMARKS_PER_FACE {
        return Err(ProcessingError::InvalidInput(format!(
            "landmark count must be between 1 and {MAX_LANDMARKS_PER_FACE}"
        )));
    }
    if face.landmarks.iter().any(|landmark| {
        !in_range(landmark.x, -0.25, 1.25)
            || !in_range(landmark.y, -0.25, 1.25)
            || !landmark.z.is_finite()
    }) {
        return Err(ProcessingError::InvalidInput(
            "landmarks must contain finite normalized coordinates".to_owned(),
        ));
    }
    if face.embedding.is_empty() || face.embedding.len() > MAX_EMBEDDING_DIMENSION {
        return Err(ProcessingError::InvalidInput(format!(
            "embedding dimension must be between 1 and {MAX_EMBEDDING_DIMENSION}"
        )));
    }
    if face.embedding.iter().any(|value| !value.is_finite()) {
        return Err(ProcessingError::InvalidInput(
            "embedding values must be finite".to_owned(),
        ));
    }
    Ok(())
}

fn validate_model_name(label: &str, value: &str) -> Result<(), ProcessingError> {
    if value.trim().is_empty() || value.len() > 128 {
        Err(ProcessingError::InvalidInput(format!(
            "{label} must contain between 1 and 128 bytes"
        )))
    } else {
        Ok(())
    }
}

fn validate_identifier(label: &str, value: &str) -> io::Result<()> {
    let valid = (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {label}"),
        ))
    }
}

fn in_range(value: f32, minimum: f32, maximum: f32) -> bool {
    value.is_finite() && value >= minimum && value <= maximum
}

fn checked_i64(value: u64, label: &str) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| format!("{label} is too large"))
}

fn nonnegative_count(value: i64) -> io::Result<u64> {
    u64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid processing count"))
}

fn nonnegative_size(value: i64) -> io::Result<u64> {
    u64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid photo byte size"))
}

fn embedding_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use daily_mirror_vision_contract::{
        ClaimRequest, FaceResult, Landmark, NormalizedBounds, PhotoAnalysisResult,
    };
    use libsql::params;

    use super::{CompleteOutcome, ProcessingError, ProcessingQueue};
    use crate::catalog::PhotoCatalog;

    struct Fixture {
        path: PathBuf,
        catalog: PhotoCatalog,
        queue: ProcessingQueue,
    }

    impl Fixture {
        async fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daily-mirror-processing-{name}-{}.db",
                std::process::id()
            ));
            let _ = tokio::fs::remove_file(&path).await;
            let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
            let queue = ProcessingQueue::new(catalog.clone());
            Self {
                path,
                catalog,
                queue,
            }
        }

        async fn ready_photo(&self, id: &str) {
            self.catalog
                .register_ready(id, &format!("photos/{id}.jpg"), 1234)
                .await
                .unwrap();
        }

        async fn cleanup(self) {
            drop(self.queue);
            drop(self.catalog);
            let _ = tokio::fs::remove_file(self.path).await;
        }
    }

    fn claim(worker_id: &str) -> ClaimRequest {
        ClaimRequest {
            worker_id: worker_id.to_owned(),
            pipeline_version: "face-v1".to_owned(),
            limit: 20,
        }
    }

    fn zero_face_result() -> PhotoAnalysisResult {
        PhotoAnalysisResult {
            oriented_width: 4656,
            oriented_height: 3496,
            original_sha256: Some("a".repeat(64)),
            processing_millis: 42,
            faces: Vec::new(),
        }
    }

    fn one_face_result() -> PhotoAnalysisResult {
        PhotoAnalysisResult {
            faces: vec![FaceResult {
                detector_confidence: 0.98,
                bounds: NormalizedBounds {
                    x: 0.2,
                    y: 0.1,
                    width: 0.5,
                    height: 0.7,
                },
                landmark_model: "mesh-v1".to_owned(),
                landmark_schema: "mesh-3d-2".to_owned(),
                landmarks: vec![Landmark {
                    x: 0.4,
                    y: 0.3,
                    z: -0.1,
                }],
                embedding_model: "recognizer-v1".to_owned(),
                embedding: vec![0.1, 0.2, 0.3],
            }],
            ..zero_face_result()
        }
    }

    #[tokio::test]
    async fn reconciliation_and_claim_are_idempotent_and_bounded() {
        let fixture = Fixture::new("claim").await;
        for index in 0..25 {
            fixture
                .ready_photo(&format!("20260830T2100{index:02}Z-claim{index:02}"))
                .await;
        }
        assert_eq!(
            fixture.queue.reconcile_missing("face-v1").await.unwrap(),
            25
        );
        assert_eq!(fixture.queue.reconcile_missing("face-v1").await.unwrap(), 0);

        let first = fixture.queue.claim(&claim("worker-a")).await.unwrap();
        assert_eq!(first.len(), 20);
        assert!(first.iter().all(|photo| photo.expected_bytes == Some(1234)));
        let second = fixture.queue.claim(&claim("worker-b")).await.unwrap();
        assert_eq!(second.len(), 5);
        let status = fixture.queue.status("face-v1").await.unwrap();
        assert_eq!((status.pending, status.leased), (0, 25));
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn expired_lease_is_reclaimed_and_old_token_cannot_complete() {
        let fixture = Fixture::new("expiry").await;
        let id = "20260830T211000Z-expiry01";
        fixture.ready_photo(id).await;
        fixture.queue.reconcile_missing("face-v1").await.unwrap();
        let old = fixture
            .queue
            .claim(&claim("worker-a"))
            .await
            .unwrap()
            .remove(0);

        let connection = fixture.catalog.connection().await.unwrap();
        connection
            .execute(
                "UPDATE photo_processing SET lease_expires_at = datetime(CURRENT_TIMESTAMP, '-1 second') WHERE photo_id = ?1",
                params![id],
            )
            .await
            .unwrap();
        let current = fixture
            .queue
            .claim(&claim("worker-b"))
            .await
            .unwrap()
            .remove(0);
        assert_ne!(old.lease_token, current.lease_token);
        assert!(matches!(
            fixture
                .queue
                .complete(id, "face-v1", &old.lease_token, &zero_face_result())
                .await,
            Err(ProcessingError::LeaseLost)
        ));
        assert_eq!(
            fixture
                .queue
                .complete(id, "face-v1", &current.lease_token, &zero_face_result())
                .await
                .unwrap(),
            CompleteOutcome::Completed
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn completion_is_per_photo_atomic_and_idempotent() {
        let fixture = Fixture::new("complete").await;
        let id = "20260830T212000Z-complete2";
        fixture.ready_photo(id).await;
        fixture.queue.enqueue_photo(id, "face-v1").await.unwrap();
        let lease = fixture
            .queue
            .claim(&claim("worker-a"))
            .await
            .unwrap()
            .remove(0);
        let result = one_face_result();

        assert_eq!(
            fixture
                .queue
                .complete(id, "face-v1", &lease.lease_token, &result)
                .await
                .unwrap(),
            CompleteOutcome::Completed
        );
        assert_eq!(
            fixture
                .queue
                .complete(id, "face-v1", &lease.lease_token, &result)
                .await
                .unwrap(),
            CompleteOutcome::AlreadyCompleted
        );
        let status = fixture.queue.status("face-v1").await.unwrap();
        assert_eq!((status.pending, status.leased, status.complete), (0, 0, 1));
        let connection = fixture.catalog.connection().await.unwrap();
        let mut rows = connection
            .query(
                "SELECT face_count, (SELECT COUNT(*) FROM faces WHERE photo_id = ?1) FROM photo_analyses WHERE photo_id = ?1",
                params![id],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<i64>(0).unwrap(), 1);
        assert_eq!(row.get::<i64>(1).unwrap(), 1);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn failed_attempt_returns_to_pending_or_stops() {
        let fixture = Fixture::new("failure").await;
        let first_id = "20260830T213000Z-retry001";
        let second_id = "20260830T213100Z-failed01";
        fixture.ready_photo(first_id).await;
        fixture.ready_photo(second_id).await;
        fixture.queue.reconcile_missing("face-v1").await.unwrap();
        let leases = fixture.queue.claim(&claim("worker-a")).await.unwrap();

        fixture
            .queue
            .fail(
                &leases[0].photo_id,
                "face-v1",
                &leases[0].lease_token,
                true,
                "driver unavailable",
            )
            .await
            .unwrap();
        fixture
            .queue
            .fail(
                &leases[1].photo_id,
                "face-v1",
                &leases[1].lease_token,
                false,
                "invalid jpeg",
            )
            .await
            .unwrap();
        let status = fixture.queue.status("face-v1").await.unwrap();
        assert_eq!((status.pending, status.failed), (1, 1));
        fixture.cleanup().await;
    }
}
