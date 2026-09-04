use std::collections::HashSet;
use std::io;

use daily_mirror_vision_contract::Landmark;
use libsql::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::processing::{ProcessingQueue, active_pipeline_version};

const RECENT_PHOTO_LIMIT: i64 = 60;
const MAX_HOUSEHOLD_MEMBERS: usize = 6;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminFaceDashboard {
    pub pipeline_version: String,
    pub queue: AdminQueueStatus,
    pub summary: AdminFaceSummary,
    pub photos: Vec<AdminPhoto>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminQueueStatus {
    pub pending: u64,
    pub leased: u64,
    pub complete: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminFaceSummary {
    pub analyzed_photos: u64,
    pub detected_faces: u64,
    pub assigned_faces: u64,
    pub unknown_faces: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminPhoto {
    pub id: String,
    pub captured_at: String,
    pub photo_url: String,
    pub thumbnail_url: String,
    pub status: String,
    pub attempt_count: u64,
    pub leased_by: Option<String>,
    pub lease_expires_at: Option<String>,
    pub last_error: Option<String>,
    pub oriented_width: Option<u32>,
    pub oriented_height: Option<u32>,
    pub processing_millis: Option<u64>,
    pub faces: Vec<AdminFace>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminFace {
    pub id: String,
    pub ordinal: u32,
    pub detector_confidence: f32,
    pub bounds: AdminBounds,
    pub landmarks: Vec<AdminLandmark>,
    pub landmark_model: String,
    pub landmark_schema: String,
    pub embedding_model: String,
    pub embedding_dimension: u32,
    pub person_id: Option<String>,
    pub person_name: Option<String>,
    pub identity_state: String,
    pub crop_url: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub struct AdminLandmark {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PeopleResponse {
    pub people: Vec<PersonFlipbook>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct PersonFlipbook {
    pub id: String,
    pub display_name: String,
    pub face_count: u64,
    pub day_count: u64,
    pub last_seen_at: Option<String>,
    pub frames: Vec<FlipbookFrame>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct FlipbookFrame {
    pub face_id: String,
    pub photo_id: String,
    pub captured_at: String,
    pub capture_day: String,
    pub detector_confidence: f32,
    pub crop_url: String,
    pub photo_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreatePersonRequest {
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreatePersonResponse {
    pub id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct HouseholdsResponse {
    pub households: Vec<HouseholdConfig>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, ToSchema)]
pub struct HouseholdConfig {
    pub id: String,
    pub display_name: String,
    pub grid_size: u32,
    pub person_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct CreateHouseholdRequest {
    pub display_name: String,
    pub grid_size: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct UpdateHouseholdRequest {
    pub display_name: String,
    pub grid_size: u32,
    pub person_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AssignFaceRequest {
    pub person_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct AssignFaceResponse {
    pub face_id: String,
    pub person_id: Option<String>,
    pub identity_state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct FaceAssignmentUpdate {
    pub face_id: String,
    pub person_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct BatchAssignFacesRequest {
    pub assignments: Vec<FaceAssignmentUpdate>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct BatchAssignFacesResponse {
    pub assignments: Vec<AssignFaceResponse>,
}

#[derive(Clone, Debug)]
pub struct FaceCrop {
    pub photo_id: String,
    pub bounds: AdminBounds,
    pub landmarks: Vec<AdminLandmark>,
}

impl ProcessingQueue {
    pub async fn admin_dashboard(&self) -> io::Result<AdminFaceDashboard> {
        let pipeline_version = active_pipeline_version()?;
        let queue = self.status(&pipeline_version).await?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let summary = dashboard_summary(&connection, &pipeline_version).await?;
        let mut rows = connection
            .query(
                "SELECT processing.photo_id, photos.captured_at, photos.media_revision,
                        CASE WHEN processing.status = 'leased'
                                  AND processing.lease_expires_at <= CURRENT_TIMESTAMP
                             THEN 'pending' ELSE processing.status END,
                        processing.attempt_count, processing.leased_by,
                        processing.lease_expires_at, processing.last_error,
                        analyses.oriented_width, analyses.oriented_height,
                        analyses.processing_millis
                 FROM photo_processing AS processing
                 JOIN photos ON photos.id = processing.photo_id
                 LEFT JOIN photo_analyses AS analyses
                   ON analyses.photo_id = processing.photo_id
                  AND analyses.pipeline_version = processing.pipeline_version
                 WHERE processing.pipeline_version = ?1
                 ORDER BY photos.captured_at DESC, processing.photo_id DESC
                 LIMIT ?2",
                params![pipeline_version.clone(), RECENT_PHOTO_LIMIT],
            )
            .await
            .map_err(io::Error::other)?;
        let mut photos = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            let media_revision = nonnegative(row.get(2).map_err(io::Error::other)?)?;
            photos.push(AdminPhoto {
                photo_url: format!("/api/photos/{id}"),
                thumbnail_url: format!("/api/photos/{id}/thumbnail?rev={media_revision}"),
                faces: self.admin_faces_for_photo(&id, &pipeline_version).await?,
                id,
                captured_at: row.get(1).map_err(io::Error::other)?,
                status: row.get(3).map_err(io::Error::other)?,
                attempt_count: nonnegative(row.get(4).map_err(io::Error::other)?)?,
                leased_by: row.get(5).map_err(io::Error::other)?,
                lease_expires_at: row.get(6).map_err(io::Error::other)?,
                last_error: row.get(7).map_err(io::Error::other)?,
                oriented_width: optional_u32(row.get(8).map_err(io::Error::other)?)?,
                oriented_height: optional_u32(row.get(9).map_err(io::Error::other)?)?,
                processing_millis: optional_u64(row.get(10).map_err(io::Error::other)?)?,
            });
        }
        Ok(AdminFaceDashboard {
            pipeline_version,
            queue: AdminQueueStatus {
                pending: queue.pending,
                leased: queue.leased,
                complete: queue.complete,
                failed: queue.failed,
            },
            summary,
            photos,
        })
    }

    pub async fn people_with_flipbooks(&self) -> io::Result<PeopleResponse> {
        let pipeline_version = active_pipeline_version()?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT people.id, people.display_name, COUNT(faces.id),
                        COUNT(DISTINCT substr(photos.captured_at, 1, 10)),
                        MAX(photos.captured_at)
                 FROM people
                 LEFT JOIN faces ON faces.person_id = people.id
                                AND faces.pipeline_version = ?1
                                AND faces.identity_state = 'confirmed'
                 LEFT JOIN photos ON photos.id = faces.photo_id
                 GROUP BY people.id, people.display_name
                 ORDER BY people.display_name COLLATE NOCASE",
                params![pipeline_version.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        let mut people = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            people.push(PersonFlipbook {
                frames: person_frames(&connection, &id, &pipeline_version).await?,
                id,
                display_name: row.get(1).map_err(io::Error::other)?,
                face_count: nonnegative(row.get(2).map_err(io::Error::other)?)?,
                day_count: nonnegative(row.get(3).map_err(io::Error::other)?)?,
                last_seen_at: row.get(4).map_err(io::Error::other)?,
            });
        }
        Ok(PeopleResponse { people })
    }

    pub async fn create_person(&self, display_name: &str) -> io::Result<CreatePersonResponse> {
        self.ensure_schema().await?;
        let display_name = validate_person_name(display_name)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.catalog.connection().await?;
        connection
            .execute(
                "INSERT INTO people (id, display_name) VALUES (?1, ?2)",
                params![id.clone(), display_name.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(CreatePersonResponse { id, display_name })
    }

    pub async fn households(&self) -> io::Result<HouseholdsResponse> {
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, display_name, grid_size
                 FROM households ORDER BY display_name COLLATE NOCASE",
                (),
            )
            .await
            .map_err(io::Error::other)?;
        let mut households = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            households.push(HouseholdConfig {
                person_ids: household_person_ids(&connection, &id).await?,
                id,
                display_name: row.get(1).map_err(io::Error::other)?,
                grid_size: u32::try_from(row.get::<i64>(2).map_err(io::Error::other)?)
                    .map_err(invalid_number)?,
            });
        }
        Ok(HouseholdsResponse { households })
    }

    pub async fn create_household(
        &self,
        display_name: &str,
        grid_size: u32,
    ) -> io::Result<HouseholdConfig> {
        self.ensure_schema().await?;
        let display_name = validate_person_name(display_name)?;
        validate_grid_size(grid_size)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.catalog.connection().await?;
        connection
            .execute(
                "INSERT INTO households (id, display_name, grid_size) VALUES (?1, ?2, ?3)",
                params![id.clone(), display_name.clone(), i64::from(grid_size)],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(HouseholdConfig {
            id,
            display_name,
            grid_size,
            person_ids: Vec::new(),
        })
    }

    pub async fn update_household(
        &self,
        household_id: &str,
        request: &UpdateHouseholdRequest,
    ) -> io::Result<HouseholdConfig> {
        validate_uuid("household ID", household_id)?;
        let display_name = validate_person_name(&request.display_name)?;
        validate_grid_size(request.grid_size)?;
        if request.person_ids.len() > request.grid_size as usize
            || request.person_ids.len() > MAX_HOUSEHOLD_MEMBERS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "household members must fit the selected grid",
            ));
        }
        let mut unique = HashSet::new();
        for person_id in &request.person_ids {
            validate_uuid("person ID", person_id)?;
            if !unique.insert(person_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "household members must be unique",
                ));
            }
        }

        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(io::Error::other)?;
        if !row_exists(
            &transaction,
            "SELECT 1 FROM households WHERE id = ?1",
            household_id,
        )
        .await?
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "household not found",
            ));
        }
        for person_id in &request.person_ids {
            if !row_exists(
                &transaction,
                "SELECT 1 FROM people WHERE id = ?1",
                person_id,
            )
            .await?
            {
                return Err(io::Error::new(io::ErrorKind::NotFound, "person not found"));
            }
        }
        transaction
            .execute(
                "UPDATE households
                 SET display_name = ?2, grid_size = ?3, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![
                    household_id,
                    display_name.clone(),
                    i64::from(request.grid_size)
                ],
            )
            .await
            .map_err(io::Error::other)?;
        transaction
            .execute(
                "DELETE FROM household_members WHERE household_id = ?1",
                params![household_id],
            )
            .await
            .map_err(io::Error::other)?;
        for (position, person_id) in request.person_ids.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO household_members (household_id, person_id, position)
                     VALUES (?1, ?2, ?3)",
                    params![household_id, person_id.clone(), position as i64],
                )
                .await
                .map_err(io::Error::other)?;
        }
        transaction.commit().await.map_err(io::Error::other)?;
        Ok(HouseholdConfig {
            id: household_id.to_owned(),
            display_name,
            grid_size: request.grid_size,
            person_ids: request.person_ids.clone(),
        })
    }

    pub async fn assign_face(
        &self,
        face_id: &str,
        person_id: Option<&str>,
    ) -> io::Result<AssignFaceResponse> {
        let response = self
            .assign_faces(&[FaceAssignmentUpdate {
                face_id: face_id.to_owned(),
                person_id: person_id.map(str::to_owned),
            }])
            .await?;
        response
            .assignments
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("face assignment returned no result"))
    }

    pub async fn assign_faces(
        &self,
        assignments: &[FaceAssignmentUpdate],
    ) -> io::Result<BatchAssignFacesResponse> {
        if assignments.is_empty() || assignments.len() > 100 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "between 1 and 100 face assignments are required",
            ));
        }
        let mut face_ids = HashSet::new();
        let mut person_ids = HashSet::new();
        for assignment in assignments {
            validate_uuid("face ID", &assignment.face_id)?;
            if !face_ids.insert(assignment.face_id.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "face assignments must be unique",
                ));
            }
            if let Some(person_id) = assignment.person_id.as_deref() {
                validate_uuid("person ID", person_id)?;
                person_ids.insert(person_id);
            }
        }
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(io::Error::other)?;
        for person_id in person_ids {
            let exists = row_exists(
                &transaction,
                "SELECT 1 FROM people WHERE id = ?1",
                person_id,
            )
            .await?;
            if !exists {
                return Err(io::Error::new(io::ErrorKind::NotFound, "person not found"));
            }
        }

        let mut responses = Vec::with_capacity(assignments.len());
        for assignment in assignments {
            let identity_state = if assignment.person_id.is_some() {
                "confirmed"
            } else {
                "unknown"
            };
            let changed = transaction
                .execute(
                    "UPDATE faces SET person_id = ?2, identity_state = ?3,
                         identity_source = CASE WHEN ?2 IS NULL THEN NULL ELSE 'manual' END,
                         identity_score = NULL, updated_at = CURRENT_TIMESTAMP
                     WHERE id = ?1",
                    params![
                        assignment.face_id.clone(),
                        assignment.person_id.clone(),
                        identity_state
                    ],
                )
                .await
                .map_err(io::Error::other)?;
            if changed != 1 {
                return Err(io::Error::new(io::ErrorKind::NotFound, "face not found"));
            }
            responses.push(AssignFaceResponse {
                face_id: assignment.face_id.clone(),
                person_id: assignment.person_id.clone(),
                identity_state: identity_state.to_owned(),
            });
        }
        transaction.commit().await.map_err(io::Error::other)?;
        Ok(BatchAssignFacesResponse {
            assignments: responses,
        })
    }

    pub async fn face_crop(&self, face_id: &str) -> io::Result<FaceCrop> {
        validate_uuid("face ID", face_id)?;
        self.ensure_schema().await?;
        let connection = self.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT photo_id, bounds_x, bounds_y, bounds_width, bounds_height,
                        landmarks_json
                 FROM faces WHERE id = ?1",
                params![face_id],
            )
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "face not found"));
        };
        Ok(FaceCrop {
            photo_id: row.get(0).map_err(io::Error::other)?,
            bounds: AdminBounds {
                x: real_to_f32(row.get(1).map_err(io::Error::other)?)?,
                y: real_to_f32(row.get(2).map_err(io::Error::other)?)?,
                width: real_to_f32(row.get(3).map_err(io::Error::other)?)?,
                height: real_to_f32(row.get(4).map_err(io::Error::other)?)?,
            },
            landmarks: decode_landmarks(row.get(5).map_err(io::Error::other)?)?,
        })
    }

    async fn admin_faces_for_photo(
        &self,
        photo_id: &str,
        pipeline_version: &str,
    ) -> io::Result<Vec<AdminFace>> {
        let connection = self.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT faces.id, faces.ordinal, faces.detector_confidence,
                        faces.bounds_x, faces.bounds_y, faces.bounds_width, faces.bounds_height,
                        faces.landmark_model, faces.landmark_schema, faces.landmarks_json,
                        faces.embedding_model, faces.embedding_dimension,
                        faces.person_id, people.display_name, faces.identity_state
                 FROM faces
                 LEFT JOIN people ON people.id = faces.person_id
                 WHERE faces.photo_id = ?1 AND faces.pipeline_version = ?2
                 ORDER BY faces.ordinal",
                params![photo_id, pipeline_version],
            )
            .await
            .map_err(io::Error::other)?;
        let mut faces = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let id: String = row.get(0).map_err(io::Error::other)?;
            faces.push(AdminFace {
                crop_url: format!("/api/admin/faces/{id}/crop"),
                id,
                ordinal: u32::try_from(row.get::<i64>(1).map_err(io::Error::other)?)
                    .map_err(invalid_number)?,
                detector_confidence: real_to_f32(row.get(2).map_err(io::Error::other)?)?,
                bounds: AdminBounds {
                    x: real_to_f32(row.get(3).map_err(io::Error::other)?)?,
                    y: real_to_f32(row.get(4).map_err(io::Error::other)?)?,
                    width: real_to_f32(row.get(5).map_err(io::Error::other)?)?,
                    height: real_to_f32(row.get(6).map_err(io::Error::other)?)?,
                },
                landmark_model: row.get(7).map_err(io::Error::other)?,
                landmark_schema: row.get(8).map_err(io::Error::other)?,
                landmarks: decode_landmarks(row.get(9).map_err(io::Error::other)?)?,
                embedding_model: row.get(10).map_err(io::Error::other)?,
                embedding_dimension: u32::try_from(row.get::<i64>(11).map_err(io::Error::other)?)
                    .map_err(invalid_number)?,
                person_id: row.get(12).map_err(io::Error::other)?,
                person_name: row.get(13).map_err(io::Error::other)?,
                identity_state: row.get(14).map_err(io::Error::other)?,
            });
        }
        Ok(faces)
    }
}

async fn dashboard_summary(
    connection: &libsql::Connection,
    pipeline_version: &str,
) -> io::Result<AdminFaceSummary> {
    let mut rows = connection
        .query(
            "SELECT
                (SELECT COUNT(*) FROM photo_analyses WHERE pipeline_version = ?1),
                (SELECT COUNT(*) FROM faces WHERE pipeline_version = ?1),
                (SELECT COUNT(*) FROM faces WHERE pipeline_version = ?1
                    AND identity_state = 'confirmed' AND person_id IS NOT NULL),
                (SELECT COUNT(*) FROM faces WHERE pipeline_version = ?1
                    AND (identity_state != 'confirmed' OR person_id IS NULL))",
            params![pipeline_version],
        )
        .await
        .map_err(io::Error::other)?;
    let row = rows
        .next()
        .await
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::other("admin summary returned no row"))?;
    Ok(AdminFaceSummary {
        analyzed_photos: nonnegative(row.get(0).map_err(io::Error::other)?)?,
        detected_faces: nonnegative(row.get(1).map_err(io::Error::other)?)?,
        assigned_faces: nonnegative(row.get(2).map_err(io::Error::other)?)?,
        unknown_faces: nonnegative(row.get(3).map_err(io::Error::other)?)?,
    })
}

async fn person_frames(
    connection: &libsql::Connection,
    person_id: &str,
    pipeline_version: &str,
) -> io::Result<Vec<FlipbookFrame>> {
    let mut rows = connection
        .query(
            "SELECT faces.id, faces.photo_id, photos.captured_at,
                    faces.detector_confidence
             FROM faces
             JOIN photos ON photos.id = faces.photo_id
             WHERE faces.person_id = ?1 AND faces.pipeline_version = ?2
               AND faces.identity_state = 'confirmed'
             ORDER BY substr(photos.captured_at, 1, 10) DESC,
                      faces.detector_confidence DESC, photos.captured_at DESC",
            params![person_id, pipeline_version],
        )
        .await
        .map_err(io::Error::other)?;
    let mut days = HashSet::new();
    let mut frames = Vec::new();
    while let Some(row) = rows.next().await.map_err(io::Error::other)? {
        let captured_at: String = row.get(2).map_err(io::Error::other)?;
        let capture_day = captured_at.get(0..10).unwrap_or(&captured_at).to_owned();
        if !days.insert(capture_day.clone()) {
            continue;
        }
        let face_id: String = row.get(0).map_err(io::Error::other)?;
        let photo_id: String = row.get(1).map_err(io::Error::other)?;
        frames.push(FlipbookFrame {
            crop_url: format!("/api/admin/faces/{face_id}/crop"),
            photo_url: format!("/api/photos/{photo_id}"),
            face_id,
            photo_id,
            captured_at,
            capture_day,
            detector_confidence: real_to_f32(row.get(3).map_err(io::Error::other)?)?,
        });
    }
    Ok(frames)
}

async fn household_person_ids(
    connection: &libsql::Connection,
    household_id: &str,
) -> io::Result<Vec<String>> {
    let mut rows = connection
        .query(
            "SELECT person_id FROM household_members
             WHERE household_id = ?1 ORDER BY position",
            params![household_id],
        )
        .await
        .map_err(io::Error::other)?;
    let mut person_ids = Vec::new();
    while let Some(row) = rows.next().await.map_err(io::Error::other)? {
        person_ids.push(row.get(0).map_err(io::Error::other)?);
    }
    Ok(person_ids)
}

async fn row_exists(
    transaction: &libsql::Transaction,
    statement: &str,
    id: &str,
) -> io::Result<bool> {
    let mut rows = transaction
        .query(statement, params![id])
        .await
        .map_err(io::Error::other)?;
    Ok(rows.next().await.map_err(io::Error::other)?.is_some())
}

fn validate_person_name(value: &str) -> io::Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 80 || value.chars().any(char::is_control) {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "display name must contain between 1 and 80 printable characters",
        ))
    } else {
        Ok(value.to_owned())
    }
}

fn validate_grid_size(value: u32) -> io::Result<()> {
    if matches!(value, 4 | 6) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "grid size must be 4 or 6",
        ))
    }
}

fn validate_uuid(label: &str, value: &str) -> io::Result<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid {label}")))
}

fn decode_landmarks(value: String) -> io::Result<Vec<AdminLandmark>> {
    serde_json::from_str::<Vec<Landmark>>(&value)
        .map_err(io::Error::other)
        .map(|landmarks| {
            landmarks
                .into_iter()
                .map(|point| AdminLandmark {
                    x: point.x,
                    y: point.y,
                    z: point.z,
                })
                .collect()
        })
}

fn nonnegative(value: i64) -> io::Result<u64> {
    u64::try_from(value).map_err(invalid_number)
}

fn optional_u64(value: Option<i64>) -> io::Result<Option<u64>> {
    value.map(nonnegative).transpose()
}

fn optional_u32(value: Option<i64>) -> io::Result<Option<u32>> {
    value
        .map(|value| u32::try_from(value).map_err(invalid_number))
        .transpose()
}

fn real_to_f32(value: f64) -> io::Result<f32> {
    let converted = value as f32;
    if converted.is_finite() {
        Ok(converted)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid real value",
        ))
    }
}

fn invalid_number(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use daily_mirror_vision_contract::{
        ClaimRequest, DEFAULT_PIPELINE_VERSION, FaceResult, Landmark, NormalizedBounds,
        PhotoAnalysisResult,
    };

    use super::{FaceAssignmentUpdate, ProcessingQueue};
    use crate::catalog::PhotoCatalog;

    #[tokio::test]
    async fn people_assignments_feed_one_best_frame_per_day() {
        let path = std::env::temp_dir().join(format!(
            "daily-mirror-face-admin-{}-{}.db",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = tokio::fs::remove_file(&path).await;
        let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
        let queue = ProcessingQueue::new(catalog.clone());
        let ids = [
            "20260830T080000Z-admin001",
            "20260830T180000Z-admin002",
            "20260831T080000Z-admin003",
        ];
        for id in ids {
            catalog
                .register_ready(id, &format!("photos/{id}.jpg"), 100)
                .await
                .unwrap();
            queue
                .enqueue_photo(id, DEFAULT_PIPELINE_VERSION)
                .await
                .unwrap();
        }
        for id in ids {
            let lease = queue
                .claim(&ClaimRequest {
                    worker_id: "admin-test".to_owned(),
                    pipeline_version: DEFAULT_PIPELINE_VERSION.to_owned(),
                    limit: 1,
                })
                .await
                .unwrap()
                .remove(0);
            queue
                .complete(id, DEFAULT_PIPELINE_VERSION, &lease.lease_token, &result())
                .await
                .unwrap();
        }
        let person = queue.create_person("Drew").await.unwrap();
        let dashboard = queue.admin_dashboard().await.unwrap();
        let face_ids = dashboard
            .photos
            .iter()
            .map(|photo| photo.faces[0].id.clone())
            .collect::<Vec<_>>();
        let invalid_batch = vec![
            FaceAssignmentUpdate {
                face_id: face_ids[0].clone(),
                person_id: Some(person.id.clone()),
            },
            FaceAssignmentUpdate {
                face_id: uuid::Uuid::new_v4().to_string(),
                person_id: Some(person.id.clone()),
            },
        ];
        let error = queue.assign_faces(&invalid_batch).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            queue
                .admin_dashboard()
                .await
                .unwrap()
                .summary
                .assigned_faces,
            0
        );

        let assignments = face_ids
            .into_iter()
            .map(|face_id| FaceAssignmentUpdate {
                face_id,
                person_id: Some(person.id.clone()),
            })
            .collect::<Vec<_>>();
        let response = queue.assign_faces(&assignments).await.unwrap();
        assert_eq!(response.assignments.len(), 3);
        let people = queue.people_with_flipbooks().await.unwrap();
        assert_eq!(people.people.len(), 1);
        assert_eq!(people.people[0].face_count, 3);
        assert_eq!(people.people[0].day_count, 2);
        assert_eq!(people.people[0].frames.len(), 2);

        drop(queue);
        drop(catalog);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn households_persist_layout_and_ordered_members() {
        let path = std::env::temp_dir().join(format!(
            "daily-mirror-household-admin-{}-{}.db",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
        let queue = ProcessingQueue::new(catalog.clone());
        let drew = queue.create_person("Drew").await.unwrap();
        let alex = queue.create_person("Alex").await.unwrap();
        let household = queue.create_household("Home", 4).await.unwrap();

        let updated = queue
            .update_household(
                &household.id,
                &super::UpdateHouseholdRequest {
                    display_name: "Mountain House".to_owned(),
                    grid_size: 6,
                    person_ids: vec![alex.id.clone(), drew.id.clone()],
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.display_name, "Mountain House");
        assert_eq!(updated.grid_size, 6);
        assert_eq!(updated.person_ids, vec![alex.id, drew.id]);

        let households = queue.households().await.unwrap();
        assert_eq!(households.households, vec![updated]);
        let invalid = queue.create_household("Nope", 5).await.unwrap_err();
        assert_eq!(invalid.kind(), std::io::ErrorKind::InvalidInput);

        drop(queue);
        drop(catalog);
        let _ = tokio::fs::remove_file(path).await;
    }

    fn result() -> PhotoAnalysisResult {
        PhotoAnalysisResult {
            oriented_width: 100,
            oriented_height: 100,
            original_sha256: None,
            processing_millis: 10,
            faces: vec![FaceResult {
                detector_confidence: 0.95,
                bounds: NormalizedBounds {
                    x: 0.2,
                    y: 0.1,
                    width: 0.5,
                    height: 0.7,
                },
                landmark_model: "yunet".to_owned(),
                landmark_schema: "yunet-5".to_owned(),
                landmarks: vec![Landmark {
                    x: 0.3,
                    y: 0.3,
                    z: 0.0,
                }],
                embedding_model: "sface".to_owned(),
                embedding: vec![0.1; 128],
            }],
        }
    }
}
