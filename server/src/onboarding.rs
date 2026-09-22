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

use crate::auth::{AuthStore, ROLE_ADMIN, ROLE_MEMBER, User, validate_household_role};
use crate::catalog::PhotoCatalog;
use crate::face_admin::{MAX_HOUSEHOLD_MEMBERS, validate_person_name};
use crate::photos::PhotoStore;
use crate::processing::{ProcessingQueue, active_pipeline_version};
#[cfg(feature = "image-processing")]
use crate::upload_flow::{FinalizeError, finalize_upload};
use crate::upload_flow::{UploadGrant, UploadRequest, upload_grant};

/// Five directions of guided capture enrol one person.
pub const REQUIRED_ENROLLMENT_PHOTOS: u32 = 5;
const ENROLLMENT_DAYS_EXEMPT: u32 = REQUIRED_ENROLLMENT_PHOTOS;
const MANUAL_MIN_DAYS: u32 = 3;

/// Whether a person in the grid has a login of their own.
pub const ACCOUNT_LINKED: &str = "linked";
pub const ACCOUNT_NONE: &str = "none";

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
    /// The signed-in account's standing here: `admin` or `member`.
    pub role: String,
    pub people: Vec<HouseholdPerson>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct HouseholdPerson {
    pub id: String,
    pub display_name: String,
    /// Set when this person is linked to an account; people in the grid who
    /// have no login of their own have no role.
    pub role: Option<String>,
    /// `linked` when this person has a user account, `none` otherwise. The
    /// invite flow hangs off this.
    pub account: String,
    pub enrollment: EnrollmentSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub struct EnrollmentSummary {
    pub enrolled: bool,
    pub enrolled_photos: u32,
    pub required_photos: u32,
}

/// Renaming a household is an administrator action.
#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct RenameHouseholdRequest {
    pub display_name: String,
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
    /// The action needs the `admin` role on the household.
    Forbidden,
    /// Deleting this account would leave a household with members but no
    /// administrator, and nobody able to add a camera or a person again.
    LastAdmin,
    PersonNotInHousehold,
    Invalid(String),
    #[cfg(feature = "image-processing")]
    Finalize(FinalizeError),
    Storage(io::Error),
}

impl OnboardingError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Disabled | Self::Forbidden => StatusCode::FORBIDDEN,
            Self::LastAdmin => StatusCode::CONFLICT,
            Self::UsernameTaken | Self::NoHousehold | Self::HouseholdFull => StatusCode::CONFLICT,
            Self::PersonNotInHousehold => StatusCode::NOT_FOUND,
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            #[cfg(feature = "image-processing")]
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
            Self::Forbidden => "Only a household administrator can do that".to_owned(),
            Self::LastAdmin => "You are the only administrator of a household that still has \
                 other accounts. Make someone else an administrator first, then delete your \
                 account."
                .to_owned(),
            Self::PersonNotInHousehold => "That person is not in your household".to_owned(),
            Self::Invalid(message) => message.clone(),
            #[cfg(feature = "image-processing")]
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
                // The account that creates a household administers it.
                Ok(()) => match auth.set_household_role(&user.id, ROLE_ADMIN).await {
                    Ok(()) => Ok(User {
                        household_id: Some(household_id),
                        person_id: Some(person_id),
                        household_role: ROLE_ADMIN.to_owned(),
                        ..user
                    }),
                    Err(error) => {
                        let _ = auth.delete_user(&user.id).await;
                        Err(storage(error))
                    }
                },
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
        role: None,
        account: ACCOUNT_NONE.to_owned(),
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
    auth: &AuthStore,
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

    // Roles live on the account, and accounts live in the auth database,
    // which is a separate file in tests. Resolve them with their own query.
    let roles = auth
        .household_account_roles(household_id)
        .await
        .map_err(storage)?;

    let mut people = Vec::with_capacity(members.len());
    for (id, display_name) in members {
        let enrollment = enrollment_summary(&connection, &pipeline, &id).await?;
        let role = roles
            .iter()
            .find(|(person_id, _)| *person_id == id)
            .map(|(_, role)| role.clone());
        people.push(HouseholdPerson {
            id,
            display_name,
            account: if role.is_some() {
                ACCOUNT_LINKED
            } else {
                ACCOUNT_NONE
            }
            .to_owned(),
            role,
            enrollment,
        });
    }
    Ok(HouseholdSummary {
        id: household_id.to_owned(),
        display_name,
        grid_size,
        self_person_id: user.person_id.clone(),
        role: user.household_role.clone(),
        people,
    })
}

/// Rename the household. Only an administrator may do this; the validation
/// matches every other 1-80 character display name on this server.
pub async fn rename_household(
    queue: &ProcessingQueue,
    auth: &AuthStore,
    user: &User,
    display_name: &str,
) -> Result<HouseholdSummary, OnboardingError> {
    let household_id = household_of(user)?.to_owned();
    if user.household_role != ROLE_ADMIN {
        return Err(OnboardingError::Forbidden);
    }
    queue
        .rename_household(&household_id, display_name)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => OnboardingError::NoHousehold,
            _ => storage(error),
        })?;
    household_for_user(queue, auth, user).await
}

// --- Account deletion ------------------------------------------------------
//
// App Review guideline 5.1.1(v): an app that lets people create an account
// must let them delete it from inside the app, not by writing to support. The
// rules below are the honest reading of a household product:
//
//   * A household is shared. Leaving one does not destroy the photographs the
//     people still in it are looking at, so an account that has housemates is
//     unseated and deleted, and the household carries on without it.
//   * An administrator who is the last one left, with other accounts still in
//     the household, is refused: unseating them would strand everybody else
//     with no way to add a person or a camera. They are told to promote
//     somebody first. (A lone administrator with no housemates is the normal
//     case and is never refused.)
//   * When the account was the last one in its household, nobody is left to
//     see any of it, so the household is erased with it: every photograph and
//     its stored object, the faces and face embeddings derived from them, the
//     people, the seating, and the cameras' claims.

/// Everything that was removed, for the log line and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeletedAccount {
    /// True when this account was the last one in its household and the
    /// household's own data was erased too.
    pub household_erased: bool,
    pub photos_deleted: u32,
    pub people_deleted: u32,
    pub devices_released: u32,
}

/// Deletes an account. See the note above for the household rules.
///
/// This is the operator side of a deletion request: no route calls it. The
/// app and the website only record a request (`account_deletion_requests`),
/// and `daily-mirror-onboarding delete-account` runs this for one of them.
/// The request row is closed at the end so the audit trail survives.
pub async fn delete_account(
    queue: &ProcessingQueue,
    auth: &AuthStore,
    store: &PhotoStore,
    user: &User,
) -> Result<DeletedAccount, OnboardingError> {
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut deleted = DeletedAccount::default();
    if let Some(household_id) = user.household_id.clone() {
        let (peers, peer_admins) = auth
            .household_peers(&household_id, &user.id)
            .await
            .map_err(storage)?;
        if peers > 0 {
            if user.household_role == ROLE_ADMIN && peer_admins == 0 {
                return Err(OnboardingError::LastAdmin);
            }
        } else {
            deleted = erase_household(queue, store, &connection, &household_id).await?;
            deleted.household_erased = true;
        }
    }
    // The pairing table binds an account to a household independently of
    // `users.household_id`, and has no foreign key to cascade.
    if table_exists(&connection, "household_users").await? {
        connection
            .execute(
                "DELETE FROM household_users WHERE user_id = ?1",
                params![user.id.clone()],
            )
            .await
            .map_err(other)?;
    }
    // Sessions, passkeys and half-finished ceremonies go with the user row.
    auth.delete_user(&user.id).await.map_err(storage)?;
    auth.mark_deletion_fulfilled(&user.id)
        .await
        .map_err(storage)?;
    Ok(deleted)
}

/// Removes a household nobody is left in: its photographs and their stored
/// objects first, because an object outliving its row is the one failure the
/// nightly reconciliation would turn back into a broken photo.
async fn erase_household(
    queue: &ProcessingQueue,
    store: &PhotoStore,
    connection: &libsql::Connection,
    household_id: &str,
) -> Result<DeletedAccount, OnboardingError> {
    let mut deleted = DeletedAccount::default();
    let has_devices = table_exists(connection, "devices").await?;
    // A household's photographs are the ones its cameras took, plus the
    // enrollment captures taken with a phone for the people seated in it.
    let mut photo_ids = Vec::new();
    let mut rows = connection
        .query(
            "SELECT id FROM photos
             WHERE enrollment_person_id IN
                   (SELECT person_id FROM household_members WHERE household_id = ?1)",
            params![household_id],
        )
        .await
        .map_err(other)?;
    while let Some(row) = rows.next().await.map_err(other)? {
        photo_ids.push(row.get::<String>(0).map_err(other)?);
    }
    drop(rows);
    if has_devices {
        let mut rows = connection
            .query(
                "SELECT id FROM photos
                 WHERE device_id IN (SELECT device_id FROM devices WHERE household_id = ?1)",
                params![household_id],
            )
            .await
            .map_err(other)?;
        while let Some(row) = rows.next().await.map_err(other)? {
            photo_ids.push(row.get::<String>(0).map_err(other)?);
        }
        drop(rows);
    }
    photo_ids.sort();
    photo_ids.dedup();
    for id in &photo_ids {
        // A missing object is not a failure here: the row is going either way.
        store.delete(id).await.map_err(storage)?;
        queue.catalog.delete(id).await.map_err(storage)?;
        queue.delete_photo(id).await.map_err(storage)?;
        deleted.photos_deleted += 1;
    }
    if has_devices {
        deleted.devices_released = connection
            .execute(
                "DELETE FROM devices WHERE household_id = ?1",
                params![household_id],
            )
            .await
            .map_err(other)? as u32;
        connection
            .execute(
                "DELETE FROM device_claim_tokens WHERE household_id = ?1",
                params![household_id],
            )
            .await
            .map_err(other)?;
    }
    deleted.people_deleted = connection
        .execute(
            "DELETE FROM people WHERE id IN
                 (SELECT person_id FROM household_members WHERE household_id = ?1)",
            params![household_id],
        )
        .await
        .map_err(other)? as u32;
    connection
        .execute(
            "DELETE FROM household_members WHERE household_id = ?1",
            params![household_id],
        )
        .await
        .map_err(other)?;
    connection
        .execute(
            "DELETE FROM households WHERE id = ?1",
            params![household_id],
        )
        .await
        .map_err(other)?;
    Ok(deleted)
}

// --- Administrator linkage -------------------------------------------------
//
// Accounts created before onboarding existed have a NULL `users.household_id`
// and therefore no household at all, even though the deployment already has
// households and people created by the face tooling. `link-household` surveys
// that state and, on request, joins the two together.

/// One household as the linking survey sees it.
#[derive(Clone, Debug, Serialize)]
pub struct HouseholdListing {
    pub id: String,
    pub display_name: String,
    pub grid_size: u32,
    /// People seated in the grid, in position order.
    pub members: Vec<String>,
    /// Usernames of the accounts linked to this household.
    pub accounts: Vec<String>,
    /// Cameras paired into this household.
    pub devices: u32,
    /// Accounts bound by the legacy `household_users` pairing table.
    pub legacy_users: u32,
}

/// A person row that could stand for this account.
#[derive(Clone, Debug, Serialize)]
pub struct PersonCandidate {
    pub id: String,
    pub display_name: String,
    /// Whether the person is already seated in a household.
    pub household_id: Option<String>,
}

/// The account's current linkage, before anything is changed.
#[derive(Clone, Debug, Serialize)]
pub struct AccountLinkage {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub household_id: Option<String>,
    pub person_id: Option<String>,
    pub role: String,
    /// The household the legacy pairing table binds this account to.
    pub legacy_household_id: Option<String>,
}

/// What `link-household` was asked to do.
#[derive(Clone, Debug, Default)]
pub struct LinkRequest {
    pub household_id: Option<String>,
    pub person_id: Option<String>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub new_person: bool,
    /// Other people who should be seated in this household, by display name.
    pub members: Vec<String>,
    /// Create any of `members` that has no `people` row yet.
    pub create_missing: bool,
}

/// The household and person the request resolves to, plus the steps that
/// applying it would take. Producing this changes nothing.
#[derive(Clone, Debug, Serialize)]
pub struct LinkPlan {
    pub household_id: String,
    pub household_name: String,
    pub person: PlannedPerson,
    pub role: String,
    pub rename_to: Option<String>,
    /// Extra people to seat, in the order they were requested.
    pub members: Vec<PlannedPerson>,
    /// Set when the grid must widen to fit everyone.
    pub grid_size_to: Option<u32>,
    pub steps: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub enum PlannedPerson {
    Existing { id: String, display_name: String },
    Create { display_name: String },
}

/// What actually changed, so a re-run can report "nothing to do".
#[derive(Clone, Debug, Serialize)]
pub struct LinkOutcome {
    pub household_id: String,
    pub person_id: String,
    pub role: String,
    pub changes: Vec<String>,
}

pub async fn survey_households(
    queue: &ProcessingQueue,
    auth: &AuthStore,
) -> Result<Vec<HouseholdListing>, OnboardingError> {
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    // `devices` and `household_users` belong to the pairing schema, which may
    // not exist yet on a deployment that has never paired a camera.
    let paired = table_exists(&connection, "devices").await?;
    let legacy = table_exists(&connection, "household_users").await?;
    let mut rows = connection
        .query(
            "SELECT id, display_name, grid_size FROM households ORDER BY created_at, id",
            (),
        )
        .await
        .map_err(other)?;
    let mut listings = Vec::new();
    while let Some(row) = rows.next().await.map_err(other)? {
        listings.push(HouseholdListing {
            id: row.get(0).map_err(other)?,
            display_name: row.get(1).map_err(other)?,
            grid_size: u32::try_from(row.get::<i64>(2).map_err(other)?).unwrap_or(0),
            members: Vec::new(),
            accounts: Vec::new(),
            devices: 0,
            legacy_users: 0,
        });
    }
    drop(rows);
    for listing in &mut listings {
        let mut rows = connection
            .query(
                "SELECT people.display_name
                 FROM household_members
                 JOIN people ON people.id = household_members.person_id
                 WHERE household_members.household_id = ?1
                 ORDER BY household_members.position",
                params![listing.id.clone()],
            )
            .await
            .map_err(other)?;
        while let Some(row) = rows.next().await.map_err(other)? {
            listing.members.push(row.get(0).map_err(other)?);
        }
        drop(rows);
        listing.accounts = auth
            .household_usernames(&listing.id)
            .await
            .map_err(storage)?;
        if paired {
            listing.devices = count_for_household(
                &connection,
                "SELECT COUNT(*) FROM devices
                 WHERE household_id = ?1 AND released_at IS NULL",
                &listing.id,
            )
            .await?;
        }
        if legacy {
            listing.legacy_users = count_for_household(
                &connection,
                "SELECT COUNT(*) FROM household_users WHERE household_id = ?1",
                &listing.id,
            )
            .await?;
        }
    }
    Ok(listings)
}

/// Current linkage for one account, including the legacy pairing binding.
pub async fn account_linkage(
    queue: &ProcessingQueue,
    user: &User,
) -> Result<AccountLinkage, OnboardingError> {
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let legacy_household_id = if table_exists(&connection, "household_users").await? {
        let mut rows = connection
            .query(
                "SELECT household_id FROM household_users WHERE user_id = ?1",
                params![user.id.clone()],
            )
            .await
            .map_err(other)?;
        match rows.next().await.map_err(other)? {
            Some(row) => Some(row.get::<String>(0).map_err(other)?),
            None => None,
        }
    } else {
        None
    };
    Ok(AccountLinkage {
        user_id: user.id.clone(),
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        household_id: user.household_id.clone(),
        person_id: user.person_id.clone(),
        role: user.household_role.clone(),
        legacy_household_id,
    })
}

/// People whose name matches the account's display name or username, ignoring
/// case. These are the rows the face tooling is likely to have created.
pub async fn person_candidates(
    queue: &ProcessingQueue,
    user: &User,
) -> Result<Vec<PersonCandidate>, OnboardingError> {
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT people.id, people.display_name,
                    (SELECT household_id FROM household_members
                     WHERE household_members.person_id = people.id LIMIT 1)
             FROM people
             WHERE people.display_name = ?1 COLLATE NOCASE
                OR people.display_name = ?2 COLLATE NOCASE
             ORDER BY people.display_name COLLATE NOCASE, people.id",
            params![user.display_name.clone(), user.username.clone()],
        )
        .await
        .map_err(other)?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next().await.map_err(other)? {
        candidates.push(PersonCandidate {
            id: row.get(0).map_err(other)?,
            display_name: row.get(1).map_err(other)?,
            household_id: row.get(2).map_err(other)?,
        });
    }
    Ok(candidates)
}

/// People whose display name matches exactly, ignoring case.
pub async fn people_named(
    queue: &ProcessingQueue,
    name: &str,
) -> Result<Vec<PersonCandidate>, OnboardingError> {
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT people.id, people.display_name,
                    (SELECT household_id FROM household_members
                     WHERE household_members.person_id = people.id LIMIT 1)
             FROM people WHERE people.display_name = ?1 COLLATE NOCASE
             ORDER BY people.id",
            params![name],
        )
        .await
        .map_err(other)?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next().await.map_err(other)? {
        candidates.push(PersonCandidate {
            id: row.get(0).map_err(other)?,
            display_name: row.get(1).map_err(other)?,
            household_id: row.get(2).map_err(other)?,
        });
    }
    Ok(candidates)
}

/// Resolve one requested member name to exactly one person, or refuse.
async fn resolve_member(
    queue: &ProcessingQueue,
    name: &str,
    create_missing: bool,
) -> Result<PlannedPerson, OnboardingError> {
    let name = validate_person_name(name).map_err(storage)?;
    match people_named(queue, &name).await?.as_slice() {
        [only] => Ok(PlannedPerson::Existing {
            id: only.id.clone(),
            display_name: only.display_name.clone(),
        }),
        [] if create_missing => Ok(PlannedPerson::Create { display_name: name }),
        [] => Err(OnboardingError::Invalid(format!(
            "no person is named \"{name}\"; pass --create-missing to create them"
        ))),
        many => Err(OnboardingError::Invalid(format!(
            "{} people are named \"{name}\"; resolve it by hand (ids: {})",
            many.len(),
            many.iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Resolve the request into an exact plan, refusing rather than guessing when
/// the choice is ambiguous. This reads only.
pub async fn plan_link_household(
    queue: &ProcessingQueue,
    auth: &AuthStore,
    user: &User,
    request: &LinkRequest,
) -> Result<LinkPlan, OnboardingError> {
    let households = survey_households(queue, auth).await?;
    let household = match request.household_id.as_deref() {
        Some(id) => households
            .iter()
            .find(|listing| listing.id == id)
            .ok_or_else(|| OnboardingError::Invalid(format!("no household {id}")))?
            .clone(),
        None => match households.as_slice() {
            [only] => only.clone(),
            [] => {
                return Err(OnboardingError::Invalid(
                    "this deployment has no households; create one first".to_owned(),
                ));
            }
            many => {
                return Err(OnboardingError::Invalid(format!(
                    "{} households exist; pass --household-id (one of: {})",
                    many.len(),
                    many.iter()
                        .map(|listing| listing.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        },
    };

    let candidates = person_candidates(queue, user).await?;
    let person = match request.person_id.as_deref() {
        Some(id) => {
            let display_name = person_name(queue, id)
                .await?
                .ok_or_else(|| OnboardingError::Invalid(format!("no person {id}")))?;
            PlannedPerson::Existing {
                id: id.to_owned(),
                display_name,
            }
        }
        None if request.new_person => PlannedPerson::Create {
            display_name: user.display_name.clone(),
        },
        None => match candidates.as_slice() {
            [only] => PlannedPerson::Existing {
                id: only.id.clone(),
                display_name: only.display_name.clone(),
            },
            [] => {
                return Err(OnboardingError::Invalid(format!(
                    "no person matches \"{}\" or \"{}\"; pass --person-id or --new-person",
                    user.display_name, user.username
                )));
            }
            many => {
                return Err(OnboardingError::Invalid(format!(
                    "{} people match; pass --person-id (one of: {})",
                    many.len(),
                    many.iter()
                        .map(|candidate| candidate.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        },
    };

    let role = match request.role.as_deref() {
        Some(role) => validate_household_role(role).map_err(storage)?,
        None if user.household_role == ROLE_ADMIN => ROLE_ADMIN.to_owned(),
        None => ROLE_MEMBER.to_owned(),
    };
    let rename_to = match request.name.as_deref() {
        Some(name) => {
            let name = validate_person_name(name).map_err(storage)?;
            (name != household.display_name).then_some(name)
        }
        None => None,
    };

    // Other members of the house, resolved the same strict way.
    let mut members = Vec::new();
    for name in &request.members {
        let planned = resolve_member(queue, name, request.create_missing).await?;
        // The account's own person is already handled above.
        let duplicate = match (&planned, &person) {
            (PlannedPerson::Existing { id, .. }, PlannedPerson::Existing { id: own, .. }) => {
                id == own
            }
            (
                PlannedPerson::Create { display_name },
                PlannedPerson::Create { display_name: own },
            ) => display_name.eq_ignore_ascii_case(own),
            _ => false,
        };
        if !duplicate
            && !members
                .iter()
                .any(|existing| same_person(existing, &planned))
        {
            members.push(planned);
        }
    }

    let linkage = account_linkage(queue, user).await?;
    let mut steps = Vec::new();
    let mut seats = household.members.len();
    match &person {
        PlannedPerson::Create { display_name } => {
            steps.push(format!(
                "create person \"{display_name}\" and seat them in {}",
                household.id
            ));
            seats += 1;
        }
        PlannedPerson::Existing { id, .. } => {
            if !is_seated(queue, &household.id, id).await? {
                steps.push(format!("seat person {id} in {}", household.id));
                seats += 1;
            }
        }
    }
    for planned in &members {
        match planned {
            PlannedPerson::Create { display_name } => {
                steps.push(format!("create person \"{display_name}\" and seat them"));
                seats += 1;
            }
            PlannedPerson::Existing { id, display_name } => {
                if !is_seated(queue, &household.id, id).await? {
                    steps.push(format!("seat \"{display_name}\" ({id})"));
                    seats += 1;
                }
            }
        }
    }
    // The grid column only accepts 4 or 6, and six is the hard ceiling.
    let grid_size_to = if seats > household.grid_size as usize {
        if seats > MAX_HOUSEHOLD_MEMBERS {
            return Err(OnboardingError::HouseholdFull);
        }
        steps.push(format!(
            "widen the grid from {} to 6 to fit {seats} people",
            household.grid_size
        ));
        Some(6)
    } else {
        if seats == household.grid_size as usize {
            steps.push(format!(
                "note: {seats} people exactly fill this {}-person grid",
                household.grid_size
            ));
        }
        None
    };
    if linkage.household_id.as_deref() != Some(household.id.as_str()) {
        steps.push(format!("set users.household_id = {}", household.id));
    }
    if let PlannedPerson::Existing { id, .. } = &person
        && linkage.person_id.as_deref() != Some(id.as_str())
    {
        steps.push(format!("set users.person_id = {id}"));
    }
    if linkage.role != role {
        steps.push(format!("set users.household_role = {role}"));
    }
    if linkage.legacy_household_id.as_deref() != Some(household.id.as_str()) {
        steps.push(format!(
            "point household_users at {} (was {})",
            household.id,
            linkage.legacy_household_id.as_deref().unwrap_or("none")
        ));
    }
    if let Some(name) = &rename_to {
        steps.push(format!(
            "rename household to \"{name}\" (was \"{}\")",
            household.display_name
        ));
    }
    if steps.is_empty() {
        steps.push("nothing to do; this account is already linked".to_owned());
    }
    Ok(LinkPlan {
        household_id: household.id,
        household_name: household.display_name,
        person,
        role,
        rename_to,
        members,
        grid_size_to,
        steps,
    })
}

fn same_person(left: &PlannedPerson, right: &PlannedPerson) -> bool {
    match (left, right) {
        (PlannedPerson::Existing { id, .. }, PlannedPerson::Existing { id: other, .. }) => {
            id == other
        }
        (
            PlannedPerson::Create { display_name },
            PlannedPerson::Create {
                display_name: other,
            },
        ) => display_name.eq_ignore_ascii_case(other),
        _ => false,
    }
}

/// Execute the plan. Every step is idempotent, so a re-run is a no-op.
///
/// The catalog and the auth store are two databases, so this cannot be one
/// transaction; the catalog seat is written transactionally and the account
/// columns follow, which is the order a partial failure can be re-run from.
pub async fn apply_link_household(
    queue: &ProcessingQueue,
    auth: &AuthStore,
    devices: &crate::devices::DeviceRegistry,
    user: &User,
    request: &LinkRequest,
) -> Result<LinkOutcome, OnboardingError> {
    let plan = plan_link_household(queue, auth, user, request).await?;
    let linkage = account_linkage(queue, user).await?;
    let mut changes = Vec::new();

    // Widen the grid before seating anyone, so the seat inserts cannot hit the
    // household-full check on the way in.
    if let Some(grid_size) = plan.grid_size_to {
        queue
            .set_grid_size(&plan.household_id, grid_size)
            .await
            .map_err(storage)?;
        changes.push(format!("widened the grid to {grid_size}"));
    }

    let person_id = match &plan.person {
        PlannedPerson::Create { display_name } => {
            let person = add_person(queue, &plan.household_id, display_name).await?;
            changes.push(format!(
                "created person {} (\"{}\")",
                person.id, person.display_name
            ));
            person.id
        }
        PlannedPerson::Existing { id, .. } => {
            if seat_person(queue, &plan.household_id, id).await? {
                changes.push(format!("seated person {id} in {}", plan.household_id));
            }
            id.clone()
        }
    };

    if linkage.household_id.as_deref() != Some(plan.household_id.as_str())
        || linkage.person_id.as_deref() != Some(person_id.as_str())
    {
        auth.link_household(&user.id, &plan.household_id, &person_id)
            .await
            .map_err(storage)?;
        changes.push(format!(
            "linked account to household {} and person {person_id}",
            plan.household_id
        ));
    }
    if linkage.role != plan.role {
        auth.set_household_role(&user.id, &plan.role)
            .await
            .map_err(storage)?;
        changes.push(format!("set role to {}", plan.role));
    }
    if linkage.legacy_household_id.as_deref() != Some(plan.household_id.as_str()) {
        devices
            .bind_user_to_household(&user.id, &plan.household_id)
            .await
            .map_err(storage)?;
        changes.push(format!(
            "moved the pairing binding to {}",
            plan.household_id
        ));
    }
    for planned in &plan.members {
        match planned {
            PlannedPerson::Create { display_name } => {
                let person = add_person(queue, &plan.household_id, display_name).await?;
                changes.push(format!(
                    "created and seated \"{}\" ({})",
                    person.display_name, person.id
                ));
            }
            PlannedPerson::Existing { id, display_name } => {
                if seat_person(queue, &plan.household_id, id).await? {
                    changes.push(format!("seated \"{display_name}\" ({id})"));
                }
            }
        }
    }
    if let Some(name) = &plan.rename_to {
        queue
            .rename_household(&plan.household_id, name)
            .await
            .map_err(storage)?;
        changes.push(format!("renamed the household to \"{name}\""));
    }
    Ok(LinkOutcome {
        household_id: plan.household_id,
        person_id,
        role: plan.role,
        changes,
    })
}

/// Seat an existing person in a household. Returns whether a seat was added.
async fn seat_person(
    queue: &ProcessingQueue,
    household_id: &str,
    person_id: &str,
) -> Result<bool, OnboardingError> {
    queue.ensure_schema().await.map_err(storage)?;
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(other)?;
    let mut rows = transaction
        .query(
            "SELECT COUNT(*), COALESCE(MAX(position), -1),
                    (SELECT grid_size FROM households WHERE id = ?1),
                    (SELECT COUNT(*) FROM household_members
                     WHERE household_id = ?1 AND person_id = ?2)
             FROM household_members WHERE household_id = ?1",
            params![household_id, person_id],
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
    let seated: i64 = row.get(3).map_err(other)?;
    drop(rows);
    if grid_size.is_none() {
        return Err(OnboardingError::NoHousehold);
    }
    if seated > 0 {
        return Ok(false);
    }
    let grid_size = grid_size.unwrap_or_default();
    if members >= grid_size || members >= MAX_HOUSEHOLD_MEMBERS as i64 {
        return Err(OnboardingError::HouseholdFull);
    }
    transaction
        .execute(
            "INSERT INTO household_members (household_id, person_id, position)
             VALUES (?1, ?2, ?3)",
            params![household_id, person_id, next_position],
        )
        .await
        .map_err(other)?;
    transaction.commit().await.map_err(other)?;
    Ok(true)
}

async fn is_seated(
    queue: &ProcessingQueue,
    household_id: &str,
    person_id: &str,
) -> Result<bool, OnboardingError> {
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM household_members WHERE household_id = ?1 AND person_id = ?2",
            params![household_id, person_id],
        )
        .await
        .map_err(other)?;
    Ok(rows.next().await.map_err(other)?.is_some())
}

async fn person_name(
    queue: &ProcessingQueue,
    person_id: &str,
) -> Result<Option<String>, OnboardingError> {
    let connection = queue.catalog.connection().await.map_err(storage)?;
    let mut rows = connection
        .query(
            "SELECT display_name FROM people WHERE id = ?1",
            params![person_id],
        )
        .await
        .map_err(other)?;
    match rows.next().await.map_err(other)? {
        Some(row) => Ok(Some(row.get(0).map_err(other)?)),
        None => Ok(None),
    }
}

async fn table_exists(
    connection: &libsql::Connection,
    name: &str,
) -> Result<bool, OnboardingError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
        )
        .await
        .map_err(other)?;
    Ok(rows.next().await.map_err(other)?.is_some())
}

async fn count_for_household(
    connection: &libsql::Connection,
    statement: &str,
    household_id: &str,
) -> Result<u32, OnboardingError> {
    let mut rows = connection
        .query(statement, params![household_id])
        .await
        .map_err(other)?;
    let Some(row) = rows.next().await.map_err(other)? else {
        return Ok(0);
    };
    Ok(u32::try_from(row.get::<i64>(0).map_err(other)?).unwrap_or(0))
}

/// Adding a person changes who the cameras recognise, so it is an
/// administrator action, like renaming. Signup makes the account that creates
/// a household its administrator, so first-run onboarding is unaffected.
pub async fn add_household_person(
    queue: &ProcessingQueue,
    user: &User,
    display_name: &str,
) -> Result<HouseholdPerson, OnboardingError> {
    let household_id = household_of(user)?.to_owned();
    if user.household_role != ROLE_ADMIN {
        return Err(OnboardingError::Forbidden);
    }
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
    // Enrollment captures come from the phone in the owner's hand, so that is
    // the default source when the app does not say otherwise.
    let capture = request
        .validated_capture("phone")
        .map_err(|error| OnboardingError::Invalid(error.to_string()))?;
    let storage_key = store
        .storage_key(&request.capture_id)
        .map_err(|error| OnboardingError::Invalid(error.to_string()))?;
    catalog
        .reserve_enrollment(
            &request.capture_id,
            &storage_key,
            request.content_length,
            person_id,
            &capture,
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

#[cfg(feature = "image-processing")]
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
                ..Default::default()
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

    /// A pre-onboarding account: it exists, but nothing points it at the
    /// households and people the face tooling already created.
    async fn legacy_setup(fixture: &Fixture, name: &str) -> (User, String, String) {
        let user = fixture
            .auth
            .create_user("drew", name, "a-good-test-password")
            .await
            .unwrap();
        let household = fixture.queue.create_household("Home", 4).await.unwrap();
        let person = fixture.queue.create_person(name).await.unwrap();
        (user, household.id, person.id)
    }

    #[tokio::test]
    async fn signup_makes_the_creating_account_an_administrator() {
        let fixture = Fixture::new("role").await;
        let user = fixture.signup("drew").await;
        assert_eq!(user.household_role, ROLE_ADMIN);
        let stored = fixture
            .auth
            .user_by_username("drew")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.household_role, ROLE_ADMIN);

        let summary = household_for_user(&fixture.queue, &fixture.auth, &stored)
            .await
            .unwrap();
        assert_eq!(summary.role, ROLE_ADMIN);
        // The person representing the account carries the account's role; a
        // person added to the grid without a login carries none.
        assert_eq!(summary.people[0].role.as_deref(), Some(ROLE_ADMIN));
        add_household_person(&fixture.queue, &stored, "Sam")
            .await
            .unwrap();
        let summary = household_for_user(&fixture.queue, &fixture.auth, &stored)
            .await
            .unwrap();
        assert_eq!(summary.people[1].role, None);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn only_an_administrator_can_rename_the_household() {
        let fixture = Fixture::new("rename").await;
        let admin = fixture.signup("drew").await;
        let summary = rename_household(&fixture.queue, &fixture.auth, &admin, "  Hirschi  ")
            .await
            .unwrap();
        assert_eq!(summary.display_name, "Hirschi");

        let member = User {
            household_role: ROLE_MEMBER.to_owned(),
            ..admin.clone()
        };
        let refused = rename_household(&fixture.queue, &fixture.auth, &member, "Nope")
            .await
            .unwrap_err();
        assert!(matches!(refused, OnboardingError::Forbidden));
        assert_eq!(refused.status_code(), StatusCode::FORBIDDEN);
        assert!(matches!(
            rename_household(&fixture.queue, &fixture.auth, &admin, "  ").await,
            Err(OnboardingError::Invalid(_))
        ));
        assert_eq!(
            household_for_user(&fixture.queue, &fixture.auth, &admin)
                .await
                .unwrap()
                .display_name,
            "Hirschi"
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn a_link_dry_run_reports_the_plan_and_changes_nothing() {
        let fixture = Fixture::new("link-dry").await;
        let (user, household_id, person_id) = legacy_setup(&fixture, "Drew").await;

        let households = survey_households(&fixture.queue, &fixture.auth)
            .await
            .unwrap();
        assert_eq!(households.len(), 1);
        assert!(households[0].members.is_empty());
        assert!(households[0].accounts.is_empty());
        assert_eq!(households[0].devices, 0);

        let candidates = person_candidates(&fixture.queue, &user).await.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, person_id);
        assert_eq!(candidates[0].household_id, None);

        let plan = plan_link_household(
            &fixture.queue,
            &fixture.auth,
            &user,
            &LinkRequest {
                role: Some("admin".to_owned()),
                name: Some("Hirschi".to_owned()),
                ..LinkRequest::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(plan.household_id, household_id);
        assert_eq!(plan.role, ROLE_ADMIN);
        assert_eq!(plan.rename_to.as_deref(), Some("Hirschi"));
        assert!(matches!(&plan.person, PlannedPerson::Existing { id, .. } if *id == person_id));
        assert_eq!(plan.steps.len(), 6, "{:?}", plan.steps);

        // Nothing was written.
        let stored = fixture
            .auth
            .user_by_username("drew")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.household_id, None);
        assert_eq!(stored.household_role, ROLE_MEMBER);
        assert_eq!(
            survey_households(&fixture.queue, &fixture.auth)
                .await
                .unwrap()[0]
                .members
                .len(),
            0
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn linking_a_pre_onboarding_account_is_idempotent() {
        let fixture = Fixture::new("link-apply").await;
        let (user, household_id, person_id) = legacy_setup(&fixture, "Drew").await;
        let devices = crate::devices::DeviceRegistry::new(fixture.queue.clone());
        let request = LinkRequest {
            role: Some("admin".to_owned()),
            name: Some("Hirschi".to_owned()),
            ..LinkRequest::default()
        };

        let outcome =
            apply_link_household(&fixture.queue, &fixture.auth, &devices, &user, &request)
                .await
                .unwrap();
        assert_eq!(outcome.household_id, household_id);
        assert_eq!(outcome.person_id, person_id);
        assert_eq!(outcome.role, ROLE_ADMIN);
        assert_eq!(outcome.changes.len(), 5, "{:?}", outcome.changes);

        let linked = fixture
            .auth
            .user_by_username("drew")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(linked.household_id.as_deref(), Some(household_id.as_str()));
        assert_eq!(linked.person_id.as_deref(), Some(person_id.as_str()));
        assert_eq!(linked.household_role, ROLE_ADMIN);
        // Device pairing now resolves to the very same household.
        assert_eq!(
            devices.household_for_user(&linked).await.unwrap(),
            household_id
        );
        let summary = household_for_user(&fixture.queue, &fixture.auth, &linked)
            .await
            .unwrap();
        assert_eq!(summary.display_name, "Hirschi");
        assert_eq!(summary.role, ROLE_ADMIN);
        assert_eq!(summary.self_person_id.as_deref(), Some(person_id.as_str()));
        assert_eq!(summary.people.len(), 1);

        // A second run finds everything already in place.
        let again =
            apply_link_household(&fixture.queue, &fixture.auth, &devices, &linked, &request)
                .await
                .unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        assert_eq!(
            household_for_user(&fixture.queue, &fixture.auth, &linked)
                .await
                .unwrap()
                .people
                .len(),
            1
        );
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn linking_seats_the_named_members_and_widens_the_grid() {
        let fixture = Fixture::new("link-members").await;
        let (user, household_id, drew) = legacy_setup(&fixture, "Drew").await;
        // The face tooling already knows these people.
        for name in ["Carson", "Dean", "Hannah"] {
            fixture.queue.create_person(name).await.unwrap();
        }
        let devices = crate::devices::DeviceRegistry::new(fixture.queue.clone());
        let request = LinkRequest {
            role: Some("admin".to_owned()),
            name: Some("Hirschi".to_owned()),
            // "Drew" is the account's own person and must not be seated twice.
            members: ["Carson", "Dean", "Drew", "Hannah"]
                .map(str::to_owned)
                .to_vec(),
            ..LinkRequest::default()
        };

        let plan = plan_link_household(&fixture.queue, &fixture.auth, &user, &request)
            .await
            .unwrap();
        assert_eq!(plan.members.len(), 3, "{:?}", plan.members);
        // Four people exactly fill the four-person grid, so no widening.
        assert_eq!(plan.grid_size_to, None);
        assert!(
            plan.steps.iter().any(|step| step.contains("exactly fill")),
            "{:?}",
            plan.steps
        );

        apply_link_household(&fixture.queue, &fixture.auth, &devices, &user, &request)
            .await
            .unwrap();
        let linked = fixture
            .auth
            .user_by_username("drew")
            .await
            .unwrap()
            .unwrap();
        let summary = household_for_user(&fixture.queue, &fixture.auth, &linked)
            .await
            .unwrap();
        assert_eq!(summary.display_name, "Hirschi");
        let seated = summary
            .people
            .iter()
            .map(|person| person.display_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(seated, ["Drew", "Carson", "Dean", "Hannah"]);
        // Only the account holder has a login; the rest are the invite seam.
        assert_eq!(summary.people[0].id, drew);
        assert_eq!(summary.people[0].account, ACCOUNT_LINKED);
        assert!(
            summary.people[1..]
                .iter()
                .all(|person| person.account == ACCOUNT_NONE && person.role.is_none())
        );
        assert_eq!(
            survey_households(&fixture.queue, &fixture.auth)
                .await
                .unwrap()[0]
                .accounts,
            ["drew"]
        );

        // A fifth person needs the six-person grid.
        let widen = LinkRequest {
            members: vec!["Robin".to_owned()],
            create_missing: true,
            ..request.clone()
        };
        let plan = plan_link_household(&fixture.queue, &fixture.auth, &linked, &widen)
            .await
            .unwrap();
        assert_eq!(plan.grid_size_to, Some(6));
        apply_link_household(&fixture.queue, &fixture.auth, &devices, &linked, &widen)
            .await
            .unwrap();
        assert_eq!(
            household_for_user(&fixture.queue, &fixture.auth, &linked)
                .await
                .unwrap()
                .people
                .len(),
            5
        );

        // An unknown name is refused rather than invented.
        assert!(matches!(
            plan_link_household(
                &fixture.queue,
                &fixture.auth,
                &linked,
                &LinkRequest {
                    members: vec!["Nobody".to_owned()],
                    ..LinkRequest::default()
                },
            )
            .await,
            Err(OnboardingError::Invalid(_))
        ));
        assert_eq!(household_id, plan.household_id);
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn linking_refuses_to_guess_when_the_choice_is_ambiguous() {
        let fixture = Fixture::new("link-ambiguous").await;
        let (user, _household_id, _person_id) = legacy_setup(&fixture, "Drew").await;
        // A second household, and a second person with the same name.
        fixture.queue.create_household("Cabin", 4).await.unwrap();
        fixture.queue.create_person("drew").await.unwrap();

        let ambiguous = plan_link_household(
            &fixture.queue,
            &fixture.auth,
            &user,
            &LinkRequest::default(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&ambiguous, OnboardingError::Invalid(message)
                if message.contains("households exist")),
            "{ambiguous:?}"
        );
        assert_eq!(ambiguous.status_code(), StatusCode::BAD_REQUEST);

        // Naming the household leaves the person ambiguous.
        let households = survey_households(&fixture.queue, &fixture.auth)
            .await
            .unwrap();
        let person_ambiguous = plan_link_household(
            &fixture.queue,
            &fixture.auth,
            &user,
            &LinkRequest {
                household_id: Some(households[0].id.clone()),
                ..LinkRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&person_ambiguous, OnboardingError::Invalid(message)
                if message.contains("people match")),
            "{person_ambiguous:?}"
        );

        // An unknown id is refused rather than silently ignored.
        assert!(matches!(
            plan_link_household(
                &fixture.queue,
                &fixture.auth,
                &user,
                &LinkRequest {
                    household_id: Some("not-a-household".to_owned()),
                    ..LinkRequest::default()
                },
            )
            .await,
            Err(OnboardingError::Invalid(_))
        ));
        fixture.cleanup().await;
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

        let summary = household_for_user(&fixture.queue, &fixture.auth, &user)
            .await
            .unwrap();
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
            household_for_user(&fixture.queue, &fixture.auth, &user).await,
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
        let summary = household_for_user(&fixture.queue, &fixture.auth, &user)
            .await
            .unwrap();
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
            capture_id: "20260915T120000Z-e0001001".to_owned(),
            content_type: "image/jpeg".to_owned(),
            content_length: 4,
            ..Default::default()
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
        enrol_photo(&fixture, &user, &person, "20260915T120000Z-e0001001", 1).await;
        enrol_photo(&fixture, &user, &person, "20260915T120100Z-e0001002", 2).await;

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
                params!["20260915T120100Z-e0001002"],
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
                &format!("20260915T1200{index:02}Z-e000100{index}"),
                1,
            )
            .await;
        }
        let status = enrollment_status(&fixture.queue, &user, &person)
            .await
            .unwrap();
        assert_eq!(status.enrolled_photos, 5);
        assert!(status.enrolled);
        let summary = household_for_user(&fixture.queue, &fixture.auth, &user)
            .await
            .unwrap();
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
