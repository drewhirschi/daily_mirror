//! Administrative device registration: the path a camera takes when it cannot
//! run the interactive claim flow, such as the original Pi rig.
//!
//! The rules that matter are that the explicit name is stored and listed, that
//! the minted token authenticates uploads exactly like a claimed device's, and
//! that re-running the command never quietly mints a second camera.

use server::{
    auth::AuthStore,
    catalog::PhotoCatalog,
    devices::{DeviceError, DeviceRegistry, RegisterDeviceRequest},
    processing::ProcessingQueue,
};

struct Fixture {
    registry: DeviceRegistry,
    household_id: String,
}

async fn fixture(label: &str) -> Fixture {
    let directory = std::env::temp_dir().join(format!(
        "daily-mirror-register-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("register.db").to_string_lossy().into_owned();
    let auth = AuthStore::local(database.clone());
    let catalog = PhotoCatalog::local(database);
    let queue = ProcessingQueue::new(catalog.clone());

    let user = auth
        .create_user("drew", "Drew", "strong-test-password")
        .await
        .unwrap();
    let household = queue.create_household("Hirschi", 4).await.unwrap();
    let person = queue.create_person("Drew").await.unwrap();
    auth.link_household(&user.id, &household.id, &person.id)
        .await
        .unwrap();

    Fixture {
        registry: DeviceRegistry::new(queue),
        household_id: household.id,
    }
}

fn request(device_id: &str, name: &str) -> RegisterDeviceRequest {
    RegisterDeviceRequest {
        device_id: device_id.to_owned(),
        device_name: name.to_owned(),
        hardware: "raspberrypi-imx519".to_owned(),
        firmware_version: "0.2.0".to_owned(),
        rotate_token: false,
    }
}

#[tokio::test]
async fn registration_stores_the_explicit_name_and_authenticates_uploads() {
    let fixture = fixture("creates").await;
    let registered = fixture
        .registry
        .register_device(&fixture.household_id, &request("a1b2c3d4e5f6", "V1 Pi"))
        .await
        .unwrap();

    // The name is the one that was asked for, not "Mirror E5F6" derived from
    // the ID the way the claim flow names a freshly paired camera.
    assert_eq!(registered.device.device_name, "V1 Pi");
    assert!(!registered.rotated);
    assert_eq!(registered.device.household_id, fixture.household_id);

    // GET /api/devices reads this list, so the household sees "V1 Pi".
    let listed = fixture.registry.list(&fixture.household_id).await.unwrap();
    let device = listed
        .iter()
        .find(|device| device.device_id == "a1b2c3d4e5f6")
        .expect("the registered device is listed");
    assert_eq!(device.device_name, "V1 Pi");
    assert_eq!(device.hardware, "raspberrypi-imx519");

    // The token is a device token everywhere else in the server.
    assert_eq!(
        fixture
            .registry
            .authenticate_device(&registered.device_token)
            .await
            .unwrap()
            .as_deref(),
        Some("a1b2c3d4e5f6"),
    );
}

#[tokio::test]
async fn re_registering_refuses_rather_than_minting_a_second_device() {
    let fixture = fixture("refuses").await;
    fixture
        .registry
        .register_device(&fixture.household_id, &request("a1b2c3d4e5f6", "V1 Pi"))
        .await
        .unwrap();

    // Same ID and name: this is the camera that is already there.
    let repeated = fixture
        .registry
        .register_device(&fixture.household_id, &request("a1b2c3d4e5f6", "V1 Pi"))
        .await;
    assert!(matches!(repeated, Err(DeviceError::InvalidInput(_))));

    // A different ID under a name the household already uses is the dangerous
    // case: it would leave two "V1 Pi" cameras and no way to tell them apart.
    let renamed = fixture
        .registry
        .register_device(&fixture.household_id, &request("ffffffffffff", "V1 Pi"))
        .await;
    assert!(matches!(renamed, Err(DeviceError::InvalidInput(_))));

    assert_eq!(
        fixture
            .registry
            .list(&fixture.household_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn rotating_replaces_the_token_and_keeps_one_device() {
    let fixture = fixture("rotates").await;
    let first = fixture
        .registry
        .register_device(&fixture.household_id, &request("a1b2c3d4e5f6", "V1 Pi"))
        .await
        .unwrap();

    let mut rotate = request("a1b2c3d4e5f6", "V1 Pi");
    rotate.rotate_token = true;
    let second = fixture
        .registry
        .register_device(&fixture.household_id, &rotate)
        .await
        .unwrap();

    assert!(second.rotated);
    assert_ne!(first.device_token, second.device_token);
    // Still one camera, and only the new token opens it.
    assert_eq!(
        fixture
            .registry
            .list(&fixture.household_id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .registry
            .authenticate_device(&second.device_token)
            .await
            .unwrap()
            .as_deref(),
        Some("a1b2c3d4e5f6"),
    );
    assert_eq!(
        fixture
            .registry
            .authenticate_device(&first.device_token)
            .await
            .unwrap(),
        None,
    );
}

#[tokio::test]
async fn a_blank_name_is_refused() {
    let fixture = fixture("blank").await;
    let refused = fixture
        .registry
        .register_device(&fixture.household_id, &request("a1b2c3d4e5f6", "   "))
        .await;
    assert!(matches!(refused, Err(DeviceError::InvalidInput(_))));
    assert!(
        fixture
            .registry
            .list(&fixture.household_id)
            .await
            .unwrap()
            .is_empty()
    );
}
