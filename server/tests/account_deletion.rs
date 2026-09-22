//! Guideline 5.1.1(v) deletion. The app only records a request through the
//! real router; `onboarding::delete_account` is what the operator's
//! `delete-account` command runs, and these tests pin down what it removes.
use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode, header},
    middleware,
};
use server::{
    auth::{AuthStore, ROLE_ADMIN, ROLE_MEMBER, User},
    catalog::PhotoCatalog,
    onboarding::{self, DeletedAccount, OnboardingError},
    passkeys::PasskeyService,
    photos::PhotoStore,
    processing::ProcessingQueue,
    view_auth,
};
use tower::ServiceExt;

struct Fixture {
    directory: std::path::PathBuf,
    auth: AuthStore,
    catalog: PhotoCatalog,
    queue: ProcessingQueue,
    store: PhotoStore,
    app: axum::Router,
}

fn fixture() -> Fixture {
    let directory =
        std::env::temp_dir().join(format!("daily-mirror-deletion-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let auth = AuthStore::local(directory.join("auth.db").to_string_lossy().into_owned());
    let catalog = PhotoCatalog::local(directory.join("catalog.db").to_string_lossy().into_owned());
    let queue = ProcessingQueue::new(catalog.clone());
    let store = PhotoStore::new(directory.join("photos"));
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(store.clone()))
        .layer(Extension(queue.clone()))
        .layer(Extension(catalog.clone()))
        .layer(Extension(auth.clone()))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            view_auth::protect,
        ));
    Fixture {
        directory,
        auth,
        catalog,
        queue,
        store,
        app,
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Creates an account through the real signup path, which also builds its
/// household, and returns the session token.
async fn sign_up(fixture: &Fixture, username: &str) -> String {
    unsafe { std::env::set_var("DAILY_MIRROR_ALLOW_SIGNUP", "1") };
    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/signup")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "username": username,
                        "display_name": username,
                        "password": "a-good-test-password",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap(),
    )
    .unwrap();
    body["token"].as_str().unwrap().to_owned()
}

fn deletion_request(method: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri("/api/auth/account/deletion-request")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// What the operator command does once a request is in: the app never
/// reaches this.
async fn fulfil(fixture: &Fixture, user: &User) -> Result<DeletedAccount, OnboardingError> {
    onboarding::delete_account(&fixture.queue, &fixture.auth, &fixture.store, user).await
}

#[tokio::test]
async fn a_deletion_request_is_recorded_once_and_deletes_nothing() {
    let fixture = fixture();
    let token = sign_up(&fixture, "asker").await;

    // Nothing requested yet, and nothing for an anonymous caller.
    assert_eq!(
        fixture
            .app
            .clone()
            .oneshot(deletion_request("GET", &token))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/account/deletion-request")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let accepted = fixture
        .app
        .clone()
        .oneshot(deletion_request("POST", &token))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    // Native sessions get the stricter header from `view_auth`.
    assert_eq!(
        accepted.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let first = body_json(accepted).await;
    assert_eq!(first["username"], "asker");
    let requested_at = first["requested_at"].as_str().unwrap().to_owned();
    assert!(requested_at.ends_with('Z') && requested_at.contains('T'));
    assert!(first.get("fulfilled_at").is_none());

    // Asking again changes nothing, and the app can read the request back.
    let again = body_json(
        fixture
            .app
            .clone()
            .oneshot(deletion_request("POST", &token))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(again["requested_at"], requested_at);
    let shown = fixture
        .app
        .clone()
        .oneshot(deletion_request("GET", &token))
        .await
        .unwrap();
    assert_eq!(shown.status(), StatusCode::OK);
    assert_eq!(body_json(shown).await["requested_at"], requested_at);

    // The account and its session are untouched; the request waits for an
    // operator, who then closes it.
    let user = fixture
        .auth
        .user_by_username("asker")
        .await
        .unwrap()
        .unwrap();
    assert!(
        fixture
            .auth
            .authenticate_session(&token)
            .await
            .unwrap()
            .is_some()
    );
    let pending = fixture.auth.pending_deletion_requests().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].username, "asker");

    fulfil(&fixture, &user).await.unwrap();
    assert!(
        fixture
            .auth
            .pending_deletion_requests()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .auth
            .authenticate_session(&token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn deleting_the_only_account_erases_its_household_and_its_photographs() {
    let fixture = fixture();
    let token = sign_up(&fixture, "solo").await;
    let user = fixture
        .auth
        .user_by_username("solo")
        .await
        .unwrap()
        .unwrap();
    let household_id = user.household_id.clone().unwrap();
    let person_id = user.person_id.clone().unwrap();

    // An enrollment capture of the person signup seated in the household.
    let photo_id = "20260919T120000Z-abcdef01";
    let jpeg = [0xff, 0xd8, 0xff, 0xd9];
    fixture.store.save(photo_id, &jpeg).await.unwrap();
    fixture
        .catalog
        .reserve_enrollment(
            photo_id,
            &fixture.store.storage_key(photo_id).unwrap(),
            jpeg.len() as u64,
            &person_id,
            &server::capture::CaptureMetadata::default(),
        )
        .await
        .unwrap();

    let deleted = fulfil(&fixture, &user).await.unwrap();
    assert!(deleted.household_erased);
    assert_eq!(deleted.photos_deleted, 1);

    assert!(
        fixture
            .auth
            .user_by_username("solo")
            .await
            .unwrap()
            .is_none()
    );
    // The session no longer authenticates anything.
    let after = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/household")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);

    // Opened directly: the catalog's own connection helper is crate-private.
    let database = libsql::Builder::new_local(fixture.directory.join("catalog.db"))
        .build()
        .await
        .unwrap();
    let connection = database.connect().unwrap();
    for (table, column, value) in [
        ("households", "id", household_id.as_str()),
        ("household_members", "household_id", household_id.as_str()),
        ("people", "id", person_id.as_str()),
        ("photos", "id", photo_id),
    ] {
        let mut rows = connection
            .query(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                libsql::params![value],
            )
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(
            count, 0,
            "{table} still holds a row for the deleted account"
        );
    }
    assert!(
        fixture.store.read(photo_id).await.unwrap().is_none(),
        "the stored object is gone, not just its row",
    );
}

#[tokio::test]
async fn a_housemate_leaves_without_taking_the_household_with_them() {
    let fixture = fixture();
    let admin_token = sign_up(&fixture, "keeper").await;
    let _ = admin_token;
    let member_token = sign_up(&fixture, "leaver").await;
    let admin = fixture
        .auth
        .user_by_username("keeper")
        .await
        .unwrap()
        .unwrap();
    let member = fixture
        .auth
        .user_by_username("leaver")
        .await
        .unwrap()
        .unwrap();
    let household_id = admin.household_id.clone().unwrap();
    // Seat the second account in the first one's household as a member.
    fixture
        .auth
        .link_household(
            &member.id,
            &household_id,
            &member.person_id.clone().unwrap(),
        )
        .await
        .unwrap();
    fixture
        .auth
        .set_household_role(&member.id, ROLE_MEMBER)
        .await
        .unwrap();

    let _ = member_token;
    // The operator command resolves the account afresh, after the linkage.
    let member = fixture.auth.user_by_id(&member.id).await.unwrap().unwrap();
    let deleted = fulfil(&fixture, &member).await.unwrap();
    assert!(!deleted.household_erased);
    assert!(
        fixture
            .auth
            .user_by_username("leaver")
            .await
            .unwrap()
            .is_none()
    );
    // The household, and the account still living in it, are untouched.
    let keeper = fixture
        .auth
        .user_by_username("keeper")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(keeper.household_id.as_deref(), Some(household_id.as_str()));
    assert_eq!(keeper.household_role, ROLE_ADMIN);
    let database = libsql::Builder::new_local(fixture.directory.join("catalog.db"))
        .build()
        .await
        .unwrap();
    let mut rows = database
        .connect()
        .unwrap()
        .query(
            "SELECT COUNT(*) FROM households WHERE id = ?1",
            libsql::params![household_id.as_str()],
        )
        .await
        .unwrap();
    let households: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(
        households, 1,
        "the household outlives the account that left"
    );
}

#[tokio::test]
async fn the_last_administrator_of_a_shared_household_is_told_to_promote_somebody() {
    let fixture = fixture();
    let admin_token = sign_up(&fixture, "onlyadmin").await;
    let member_token = sign_up(&fixture, "housemate").await;
    let _ = member_token;
    let admin = fixture
        .auth
        .user_by_username("onlyadmin")
        .await
        .unwrap()
        .unwrap();
    let member = fixture
        .auth
        .user_by_username("housemate")
        .await
        .unwrap()
        .unwrap();
    let household_id = admin.household_id.clone().unwrap();
    fixture
        .auth
        .link_household(
            &member.id,
            &household_id,
            &member.person_id.clone().unwrap(),
        )
        .await
        .unwrap();
    fixture
        .auth
        .set_household_role(&member.id, ROLE_MEMBER)
        .await
        .unwrap();

    let _ = admin_token;
    let refused = fulfil(&fixture, &admin).await.unwrap_err();
    assert_eq!(refused.status_code(), StatusCode::CONFLICT);
    assert!(
        fixture
            .auth
            .user_by_username("onlyadmin")
            .await
            .unwrap()
            .is_some(),
        "a refused deletion leaves the account alone",
    );

    // Promoting the housemate clears the way.
    fixture
        .auth
        .set_household_role(&member.id, ROLE_ADMIN)
        .await
        .unwrap();
    fulfil(&fixture, &admin).await.unwrap();
    assert!(
        fixture
            .auth
            .user_by_username("onlyadmin")
            .await
            .unwrap()
            .is_none()
    );
}
