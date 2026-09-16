//! Household onboarding and guided face enrollment.
//!
//! The HTTP routes and the `daily-mirror-onboarding` CLI are thin adapters
//! over these functions, so signup, membership checks and enrollment status
//! behave identically however they are reached.
use std::io;

use axum::http::StatusCode;
use libsql::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::{AuthStore, User};
use crate::catalog::PhotoCatalog;
use crate::face_admin::{MAX_HOUSEHOLD_MEMBERS, validate_person_name};
use crate::photos::PhotoStore;
use crate::processing::{ProcessingQueue, active_pipeline_version};
use crate::upload_flow::{
    FinalizeError, UploadGrant, UploadRequest, finalize_upload, upload_grant,
};

/// Five directions of guided capture enrol one person.
pub const REQUIRED_ENROLLMENT_PHOTOS: u32 = 5;
const ENROLLMENT_DAYS_EXEMPT: u32 = REQUIRED_ENROLLMENT_PHOTOS;
const MANUAL_MIN_DAYS: u32 = 3;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct SignupRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct HouseholdSummary {
    pub id: String,
    pub display_name: String,
    pub grid_size: u32,
    /// The member representing the signed-in user, when onboarding created one.
    pub self_person_id: Option<String>,
    pub people: Vec<HouseholdPerson>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct HouseholdPerson {
    pub id: String,
    pub display_name: String,
    pub enrollment: EnrollmentSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub struct EnrollmentSummary {
    pub enrolled: bool,
    pub enrolled_photos: u32,
    pub required_photos: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EnrollmentStatus {
    pub person_id: String,
    pub enrolled: bool,
    pub enrolled_photos: u32,
    pub required_photos: u32,
    pub photos: Vec<EnrollmentPhoto>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct EnrollmentPhoto {
    pub photo_id: String,
    pub captured_at: String,
    /// `uploading`, `processing`, `enrolled`, `retake` or `failed`.
    pub status: String,
    pub face_count: Option<u32>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug)]
pub enum OnboardingError {
    Disabled,
    UsernameTaken,
    NoHousehold,
    HouseholdFull,
    PersonNotInHousehold,
    Invalid(String),
    Finalize(FinalizeError),
    Storage(io::Error),
}

impl OnboardingError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Disabled => StatusCode::FORBIDDEN,
            Self::UsernameTaken | Self::NoHousehold | Self::HouseholdFull => StatusCode::CONFLICT,
            Self::PersonNotInHousehold => StatusCode::NOT_FOUND,
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Finalize(error) => error.status_code(),
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Disabled => "Signup is disabled on this server".to_owned(),
            Self::UsernameTaken => "That username is already taken".to_owned(),
            Self::NoHousehold => {
                "This account has no household. Sign up again or ask an administrator to link one."
                    .to_owned()
            }
            Self::HouseholdFull => "This household is already full".to_owned(),
            Self::PersonNotInHousehold => "That person is not in your household".to_owned(),
            Self::Invalid(message) => message.clone(),
            Self::Finalize(error) => error.to_string(),
            Self::Storage(_) => "Onboarding service unavailable".to_owned(),
        }
    }
}

impl std::fmt::Display for OnboardingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for OnboardingError {}

impl From<OnboardingError> for io::Error {
    fn from(error: OnboardingError) -> Self {
        match error {
            OnboardingError::Storage(inner) => inner,
            OnboardingError::Invalid(message) => {
                io::Error::new(io::ErrorKind::InvalidInput, message)
            }
            other => io::Error::other(other.message()),
        }
    }
}

fn storage(error: io::Error) -> OnboardingError {
    match error.kind() {
        io::ErrorKind::InvalidInput => OnboardingError::Invalid(error.to_string()),
        _ => OnboardingError::Storage(error),
    }
}

/// Routes surface the same message the CLI prints.
pub fn http_error(error: OnboardingError) -> axum::response::Response {
    crate::auth_http::error(error.status_code(), &error.message())
}

pub fn signup_enabled() -> bool {
    std::env::var("DAILY_MIRROR_ALLOW_SIGNUP").as_deref() == Ok("1")
}

/// Creates the account, its household, and the person representing the new
/// user, then links them. A later failure removes the account so the username
/// stays available for a retry.
pub async fn signup(
    auth: &AuthStore,
    queue: &ProcessingQueue,
    request: &SignupRequest,
) -> Result<User, OnboardingError> {
    let user = auth
        .create_user(&request.username, &request.display_name, &request.password)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::AlreadyExists => OnboardingError::UsernameTaken,
            io::ErrorKind::InvalidInput => OnboardingError::Invalid(error.to_string()),
            _ => OnboardingError::Storage(error),
        })?;
    match link_new_household(queue, &user).await {
        Ok((household_id, person_id)) => {
            match auth
                .link_household(&user.id, &household_id, &person_id)
                .await
            {
                Ok(()) => Ok(User {
                    household_id: Some(household_id),
                    person_id: Some(person_id),
                    ..user
                }),
                Err(error) => {
                    let _ = auth.delete_user(&user.id).await;
                    Err(storage(error))
                }
            }
        }
        Err(error) => {
            let _ = auth.delete_user(&user.id).await;
            Err(error)
        }
    }
}

async fn link_new_household(
    queue: &ProcessingQueue,
    user: &User,
) -> Result<(String, String), OnboardingError> {
    let household = queue
        .create_household(&format!("{}'s home", user.display_name), 4)
        .await
        .map_err(storage)?;
    let person = add_person(queue, &household.id, &user.display_name).await?;
    Ok((household.id, person.id))
}

/// Creates a person and seats them in the household in one transaction, so a
/// full grid never leaves an unreachable person record behind.
async fn add_person(
    queue: &ProcessingQueue,
    household_id: &str,
    display_name: &str,
) -> Result<HouseholdPerson, OnboardingError> {
    let display_name = validate_person_name(display_name).map_err(storage)?;
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(other)?;
    let mut rows = transaction
        .query(
            "SELECT COUNT(*), COALESCE(MAX(position), -1),
                    (SELECT grid_size FROM households WHERE id = ?1)
             FROM household_members WHERE household_id = ?1",
            params![household_id],
        )
        .await
        .map_err(other)?;
    let row =
        rows.next().await.map_err(other)?.ok_or_else(|| {
            OnboardingError::Storage(io::Error::other("household count is empty"))
        })?;
    let members: i64 = row.get(0).map_err(other)?;
    let next_position: i64 = row.get::<i64>(1).map_err(other)? + 1;
    let grid_size: Option<i64> = row.get(2).map_err(other)?;
    drop(rows);
    let Some(grid_size) = grid_size else {
        return Err(OnboardingError::NoHousehold);
    };
    if members >= grid_size || members >= MAX_HOUSEHOLD_MEMBERS as i64 {
        return Err(OnboardingError::HouseholdFull);
    }
    let id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO people (id, display_name) VALUES (?1, ?2)",
            params![id.clone(), display_name.clone()],
        )
        .await
        .map_err(other)?;
    transaction
        .execute(
            "INSERT INTO household_members (household_id, person_id, position)
             VALUES (?1, ?2, ?3)",
            params![household_id, id.clone(), next_position],
        )
        .await
        .map_err(other)?;
    transaction.commit().await.map_err(other)?;
    Ok(HouseholdPerson {
        id,
        display_name,
        enrollment: EnrollmentSummary {
            enrolled: false,
            enrolled_photos: 0,
            required_photos: REQUIRED_ENROLLMENT_PHOTOS,
        },
    })
}

fn other(error: impl std::fmt::Display) -> OnboardingError {
    OnboardingError::Storage(io::Error::other(error.to_string()))
}

fn household_of(user: &User) -> Result<&str, OnboardingError> {
    user.household_id
        .as_deref()
        .ok_or(OnboardingError::NoHousehold)
}

pub async fn household_for_user(
    queue: &ProcessingQueue,
    user: &User,
) -> Result<HouseholdSummary, OnboardingError> {
    let household_id = household_of(user)?;
    queue.ensure_schema().await.map_err(storage)?;
    let pipeline = active_pipeline_version().map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT display_name, grid_size FROM households WHERE id = ?1",
            params![household_id],
        )
        .await
        .map_err(other)?;
    let Some(row) = rows.next().await.map_err(other)? else {
        return Err(OnboardingError::NoHousehold);
    };
    let display_name: String = row.get(0).map_err(other)?;
    let grid_size = u32::try_from(row.get::<i64>(1).map_err(other)?)
        .map_err(|_| OnboardingError::Storage(io::Error::other("invalid grid size")))?;
    drop(rows);

    let mut rows = connection
        .query(
            "SELECT people.id, people.display_name
             FROM household_members
             JOIN people ON people.id = household_members.person_id
             WHERE household_members.household_id = ?1
             ORDER BY household_members.position",
            params![household_id],
        )
        .await
        .map_err(other)?;
    let mut members = Vec::new();
    while let Some(row) = rows.next().await.map_err(other)? {
        members.push((
            row.get::<String>(0).map_err(other)?,
            row.get::<String>(1).map_err(other)?,
        ));
    }
    drop(rows);

    let mut people = Vec::with_capacity(members.len());
    for (id, display_name) in members {
        let enrollment = enrollment_summary(&connection, &pipeline, &id).await?;
        people.push(HouseholdPerson {
            id,
            display_name,
            enrollment,
        });
    }
    Ok(HouseholdSummary {
        id: household_id.to_owned(),
        display_name,
        grid_size,
        self_person_id: user.person_id.clone(),
        people,
    })
}

pub async fn add_household_person(
    queue: &ProcessingQueue,
    user: &User,
    display_name: &str,
) -> Result<HouseholdPerson, OnboardingError> {
    let household_id = household_of(user)?.to_owned();
    add_person(queue, &household_id, display_name).await
}

async fn require_membership(
    queue: &ProcessingQueue,
    user: &User,
    person_id: &str,
) -> Result<(), OnboardingError> {
    let household_id = household_of(user)?;
    if Uuid::parse_str(person_id).is_err() {
        return Err(OnboardingError::PersonNotInHousehold);
    }
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM household_members WHERE household_id = ?1 AND person_id = ?2",
            params![household_id, person_id],
        )
        .await
        .map_err(other)?;
    if rows.next().await.map_err(other)?.is_none() {
        return Err(OnboardingError::PersonNotInHousehold);
    }
    Ok(())
}

pub async fn create_enrollment_upload(
    store: &PhotoStore,
    catalog: &PhotoCatalog,
    queue: &ProcessingQueue,
    user: &User,
    person_id: &str,
    request: &UploadRequest,
) -> Result<UploadGrant, OnboardingError> {
    require_membership(queue, user, person_id).await?;
    let storage_key = store
        .storage_key(&request.capture_id)
        .map_err(|error| OnboardingError::Invalid(error.to_string()))?;
    catalog
        .reserve_enrollment(
            &request.capture_id,
            &storage_key,
            request.content_length,
            person_id,
        )
        .await
        .map_err(storage)?;
    let target = store
        .create_upload(
            &request.capture_id,
            &request.content_type,
            request.content_length,
        )
        .await
        .map_err(storage)?;
    Ok(upload_grant(
        target,
        format!(
            "/api/household/people/{person_id}/enrollment/uploads/{}",
            request.capture_id
        ),
    ))
}

pub async fn finalize_enrollment_upload(
    store: &PhotoStore,
    catalog: &PhotoCatalog,
    queue: &ProcessingQueue,
    user: &User,
    person_id: &str,
    photo_id: &str,
) -> Result<(), OnboardingError> {
    require_membership(queue, user, person_id).await?;
    finalize_upload(store, catalog, queue, photo_id)
        .await
        .map_err(OnboardingError::Finalize)
}

pub async fn enrollment_status(
    queue: &ProcessingQueue,
    user: &User,
    person_id: &str,
) -> Result<EnrollmentStatus, OnboardingError> {
    require_membership(queue, user, person_id).await?;
    let pipeline = active_pipeline_version().map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let summary = enrollment_summary(&connection, &pipeline, person_id).await?;
    let mut rows = connection
        .query(
            "SELECT photos.id, photos.captured_at, photos.status,
                    photos.thumbnail_status, photos.media_revision,
                    processing.status, analyses.face_count,
                    (SELECT COUNT(*) FROM faces
                       WHERE faces.photo_id = photos.id
                         AND faces.pipeline_version = ?2
                         AND faces.person_id = ?1
                         AND faces.identity_state = 'confirmed'
                         AND faces.identity_source = 'enrollment')
             FROM photos
             LEFT JOIN photo_processing AS processing
               ON processing.photo_id = photos.id AND processing.pipeline_version = ?2
             LEFT JOIN photo_analyses AS analyses
               ON analyses.photo_id = photos.id AND analyses.pipeline_version = ?2
             WHERE photos.enrollment_person_id = ?1
             ORDER BY photos.captured_at, photos.id",
            params![person_id, pipeline.clone()],
        )
        .await
        .map_err(other)?;
    let mut photos = Vec::new();
    while let Some(row) = rows.next().await.map_err(other)? {
        let photo_id: String = row.get(0).map_err(other)?;
        let captured_at: String = row.get(1).map_err(other)?;
        let photo_status: String = row.get(2).map_err(other)?;
        let thumbnail_status: String = row.get(3).map_err(other)?;
        let media_revision: i64 = row.get(4).map_err(other)?;
        let processing_status: Option<String> = row.get(5).map_err(other)?;
        let analyzed_faces: Option<i64> = row.get(6).map_err(other)?;
        let enrolled_faces: i64 = row.get(7).map_err(other)?;
        let status = photo_state(
            &photo_status,
            processing_status.as_deref(),
            enrolled_faces > 0,
        );
        photos.push(EnrollmentPhoto {
            thumbnail_url: (photo_status == "ready" && thumbnail_status == "ready")
                .then(|| format!("/api/photos/{photo_id}/thumbnail?rev={media_revision}")),
            photo_id,
            captured_at,
            face_count: match status {
                "enrolled" | "retake" => analyzed_faces.and_then(|count| u32::try_from(count).ok()),
                _ => None,
            },
            status: status.to_owned(),
        });
    }
    Ok(EnrollmentStatus {
        person_id: person_id.to_owned(),
        enrolled: summary.enrolled,
        enrolled_photos: summary.enrolled_photos,
        required_photos: summary.required_photos,
        photos,
    })
}

/// The photo row leads: it is `pending` until the object is verified. After
/// that the processing row decides, and a complete analysis is only enrolled
/// when the single detected face was attached to this person.
fn photo_state(
    photo_status: &str,
    processing_status: Option<&str>,
    enrolled_face: bool,
) -> &'static str {
    if photo_status != "ready" {
        return "uploading";
    }
    match processing_status {
        Some("complete") if enrolled_face => "enrolled",
        Some("complete") => "retake",
        Some("failed") => "failed",
        _ => "processing",
    }
}

/// Counts the evidence teaching this person's profile. Enrollment photos
/// enrol on their own; manual photos still need three capture days.
async fn enrollment_summary(
    connection: &libsql::Connection,
    pipeline: &str,
    person_id: &str,
) -> Result<EnrollmentSummary, OnboardingError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(DISTINCT CASE WHEN faces.identity_source = 'enrollment'
                        THEN faces.photo_id END),
                    COUNT(DISTINCT faces.photo_id),
                    COUNT(DISTINCT substr(photos.captured_at, 1, 10))
             FROM faces JOIN photos ON photos.id = faces.photo_id
             WHERE faces.person_id = ?1 AND faces.pipeline_version = ?2
               AND faces.identity_state = 'confirmed'
               AND faces.identity_source IN ('manual', 'enrollment')",
            params![person_id, pipeline],
        )
        .await
        .map_err(other)?;
    let row =
        rows.next().await.map_err(other)?.ok_or_else(|| {
            OnboardingError::Storage(io::Error::other("enrollment count is empty"))
        })?;
    let enrolled_photos = count(row.get(0).map_err(other)?)?;
    let photos = count(row.get(1).map_err(other)?)?;
    let days = count(row.get(2).map_err(other)?)?;
    Ok(EnrollmentSummary {
        enrolled: enrolled_photos >= ENROLLMENT_DAYS_EXEMPT
            || (photos >= REQUIRED_ENROLLMENT_PHOTOS && days >= MANUAL_MIN_DAYS),
        enrolled_photos,
        required_photos: REQUIRED_ENROLLMENT_PHOTOS,
    })
}

fn count(value: i64) -> Result<u32, OnboardingError> {
    u32::try_from(value)
        .map_err(|_| OnboardingError::Storage(io::Error::other("invalid enrollment count")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use daily_mirror_vision_contract::{
        FaceResult, Landmark, NormalizedBounds, PhotoAnalysisResult,
    };

    struct Fixture {
        root: std::path::PathBuf,
        auth: AuthStore,
        catalog: PhotoCatalog,
        queue: ProcessingQueue,
        store: PhotoStore,
    }

    impl Fixture {
        async fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("daily-mirror-onboarding-{name}-{}", Uuid::new_v4()));
            tokio::fs::create_dir_all(&root).await.unwrap();
            let catalog =
                PhotoCatalog::local(root.join("catalog.db").to_string_lossy().into_owned());
            Self {
                auth: AuthStore::local(root.join("auth.db").to_string_lossy().into_owned()),
                queue: ProcessingQueue::new(catalog.clone()),
                store: PhotoStore::new(root.join("photos")),
                catalog,
                root,
            }
        }

        async fn signup(&self, username: &str) -> User {
            signup(
                &self.auth,
                &self.queue,
                &SignupRequest {
                    username: username.to_owned(),
                    display_name: "Drew".to_owned(),
                    password: "a-good-test-password".to_owned(),
                },
            )
            .await
            .unwrap()
        }

        async fn cleanup(self) {
            drop(self.queue);
            drop(self.catalog);
            drop(self.auth);
            let _ = tokio::fs::remove_dir_all(self.root).await;
        }
    }

    fn analysis(faces: usize) -> PhotoAnalysisResult {
        PhotoAnalysisResult {
            oriented_width: 800,
            oriented_height: 600,
            original_sha256: Some("b".repeat(64)),
            processing_millis: 12,
            faces: (0..faces)
                .map(|index| FaceResult {
                    detector_confidence: 0.9,
                    bounds: NormalizedBounds {
                        x: 0.1 * index as f32,
                        y: 0.1,
                        width: 0.3,
                        height: 0.3,
                    },
                    landmark_model: "mesh-v1".to_owned(),
                    landmark_schema: "mesh-3d-2".to_owned(),
                    landmarks: vec![Landmark {
                        x: 0.4,
                        y: 0.3,
                        z: 0.0,
                    }],
                    embedding_model: "sface".to_owned(),
                    embedding: vec![1.0, 0.0],
                })
                .collect(),
        }
    }

    /// Runs a captured enrollment photo all the way through processing.
    async fn enrol_photo(fixture: &Fixture, user: &User, person: &str, id: &str, faces: usize) {
        let grant = create_enrollment_upload(
            &fixture.store,
            &fixture.catalog,
            &fixture.queue,
            user,
            person,
            &UploadRequest {
                capture_id: id.to_owned(),
                content_type: "image/jpeg".to_owned(),
                content_length: 4,
            },
        )
        .await
        .unwrap();
        assert!(grant.complete_url.ends_with(id));
        fixture.catalog.mark_ready(id).await.unwrap();
        let pipeline = active_pipeline_version().unwrap();
        fixture.queue.enqueue_photo(id, &pipeline).await.unwrap();
        let lease = fixture
            .queue
            .claim(&daily_mirror_vision_contract::ClaimRequest {
                worker_id: "test-worker".to_owned(),
                pipeline_version: pipeline.clone(),
                limit: 1,
            })
            .await
            .unwrap()
            .remove(0);
        fixture
            .queue
            .complete(id, &pipeline, &lease.lease_token, &analysis(faces))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn signup_creates_a_linked_household_and_person() {
        let fixture = Fixture::new("signup").await;
        let user = fixture.signup("drew").await;
        let household_id = user.household_id.clone().unwrap();
        let person_id = user.person_id.clone().unwrap();

        let stored = fixture
            .auth
            .user_by_username("drew")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.household_id, Some(household_id.clone()));
        assert_eq!(stored.person_id, Some(person_id.clone()));

        let summary = household_for_user(&fixture.queue, &user).await.unwrap();
        assert_eq!(summary.id, household_id);
        assert_eq!(summary.self_person_id, Some(person_id.clone()));
        assert_eq!(summary.people.len(), 1);
        assert_eq!(summary.people[0].id, person_id);
        assert_eq!(summary.people[0].display_name, "Drew");
        assert!(!summary.people[0].enrollment.enrolled);
        assert_eq!(summary.people[0].enrollment.required_photos, 5);

        assert!(matches!(
            signup(
                &fixture.auth,
                &fixture.queue,
                &SignupRequest {
                    username: "drew".to_owned(),
                    display_name: "Drew".to_owned(),
                    password: "a-good-test-password".to_owned(),
                },
            )
            .await,
            Err(OnboardingError::UsernameTaken)
        ));
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_user_without_a_household_is_told_to_sign_up_again() {
        let fixture = Fixture::new("unlinked").await;
        let user = fixture
            .auth
            .create_user("solo", "Solo", "a-good-test-password")
            .await
            .unwrap();
        assert!(matches!(
            household_for_user(&fixture.queue, &user).await,
            Err(OnboardingError::NoHousehold)
        ));
        assert!(matches!(
            add_household_person(&fixture.queue, &user, "Guest").await,
            Err(OnboardingError::NoHousehold)
        ));
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn adding_people_stops_at_the_household_grid() {
        let fixture = Fixture::new("full").await;
        let user = fixture.signup("drew").await;
        // Signup seats the user, so three more fill a four-person grid.
        for name in ["Sam", "Ada", "Kit"] {
            add_household_person(&fixture.queue, &user, name)
                .await
                .unwrap();
        }
        assert!(matches!(
            add_household_person(&fixture.queue, &user, "Overflow").await,
            Err(OnboardingError::HouseholdFull)
        ));
        let summary = household_for_user(&fixture.queue, &user).await.unwrap();
        assert_eq!(summary.people.len(), 4);
        assert!(
            !summary
                .people
                .iter()
                .any(|person| person.display_name == "Overflow")
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn enrollment_uploads_require_the_person_to_be_in_the_household() {
        let fixture = Fixture::new("membership").await;
        let user = fixture.signup("drew").await;
        let outsider = fixture.queue.create_person("Stranger").await.unwrap();
        let request = UploadRequest {
            capture_id: "20260915T120000Z-enrol001".to_owned(),
            content_type: "image/jpeg".to_owned(),
            content_length: 4,
        };
        assert!(matches!(
            create_enrollment_upload(
                &fixture.store,
                &fixture.catalog,
                &fixture.queue,
                &user,
                &outsider.id,
                &request,
            )
            .await,
            Err(OnboardingError::PersonNotInHousehold)
        ));
        assert!(matches!(
            enrollment_status(&fixture.queue, &user, &outsider.id).await,
            Err(OnboardingError::PersonNotInHousehold)
        ));
        // A reservation must not exist for a rejected request.
        assert!(
            fixture
                .catalog
                .expected_size(&request.capture_id)
                .await
                .unwrap()
                .is_none()
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn one_face_enrols_and_two_faces_ask_for_a_retake() {
        let fixture = Fixture::new("faces").await;
        let user = fixture.signup("drew").await;
        let person = user.person_id.clone().unwrap();
        enrol_photo(&fixture, &user, &person, "20260915T120000Z-enrol001", 1).await;
        enrol_photo(&fixture, &user, &person, "20260915T120100Z-enrol002", 2).await;

        let status = enrollment_status(&fixture.queue, &user, &person)
            .await
            .unwrap();
        assert_eq!(status.photos.len(), 2);
        assert_eq!(status.photos[0].status, "enrolled");
        assert_eq!(status.photos[0].face_count, Some(1));
        assert!(status.photos[0].thumbnail_url.is_none());
        assert_eq!(status.photos[1].status, "retake");
        assert_eq!(status.photos[1].face_count, Some(2));
        assert_eq!(status.enrolled_photos, 1);
        assert!(!status.enrolled);

        // The retake photo's faces stay unassigned for manual review.
        let connection = fixture.catalog.connection().await.unwrap();
        let mut rows = connection
            .query(
                "SELECT COUNT(*) FROM faces WHERE photo_id = ?1
                 AND identity_state = 'unknown' AND person_id IS NULL",
                params!["20260915T120100Z-enrol002"],
            )
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            2
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn five_enrollment_photos_mark_a_person_enrolled() {
        let fixture = Fixture::new("enrolled").await;
        let user = fixture.signup("drew").await;
        let person = user.person_id.clone().unwrap();
        for index in 0..5 {
            enrol_photo(
                &fixture,
                &user,
                &person,
                &format!("20260915T1200{index:02}Z-enrol00{index}"),
                1,
            )
            .await;
        }
        let status = enrollment_status(&fixture.queue, &user, &person)
            .await
            .unwrap();
        assert_eq!(status.enrolled_photos, 5);
        assert!(status.enrolled);
        let summary = household_for_user(&fixture.queue, &user).await.unwrap();
        assert!(summary.people[0].enrollment.enrolled);
        fixture.cleanup().await;
    }

    #[test]
    fn photo_states_follow_the_upload_then_processing_row() {
        assert_eq!(photo_state("pending", None, false), "uploading");
        assert_eq!(photo_state("pending", Some("complete"), true), "uploading");
        assert_eq!(photo_state("ready", None, false), "processing");
        assert_eq!(photo_state("ready", Some("pending"), false), "processing");
        assert_eq!(photo_state("ready", Some("leased"), false), "processing");
        assert_eq!(photo_state("ready", Some("complete"), true), "enrolled");
        assert_eq!(photo_state("ready", Some("complete"), false), "retake");
        assert_eq!(photo_state("ready", Some("failed"), false), "failed");
    }
}
