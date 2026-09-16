use std::io;

use axum::http::{HeaderMap, StatusCode, header};

use crate::devices::DeviceRegistry;
use crate::photos::PhotoStore;

/// Who is uploading. Per-device tokens are the pairing-era credential; the
/// shared token is the Pi rig's legacy path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadPrincipal {
    /// A device that redeemed a claim token. The photo row records its ID, and
    /// through it the household.
    Device(String),
    /// The deployment-wide `DAILY_MIRROR_UPLOAD_TOKEN`.
    ///
    /// TODO(pairing): remove once the last Pi rig is re-provisioned with a
    /// per-device token. `docs/device-pairing-plan.md` retires the shared
    /// token; it survives only so the current rig keeps uploading.
    SharedToken,
}

impl UploadPrincipal {
    pub fn device_id(&self) -> Option<&str> {
        match self {
            Self::Device(device_id) => Some(device_id),
            Self::SharedToken => None,
        }
    }
}

/// Accept either a per-device bearer token or the shared upload token.
///
/// A device token wins when both could match, and authenticating one records
/// the device as seen.
pub async fn authorize(
    registry: &DeviceRegistry,
    headers: &HeaderMap,
) -> Result<UploadPrincipal, StatusCode> {
    let supplied = bearer(headers);
    if let Some(token) = supplied
        && let Some(device_id) = registry
            .authenticate_device(token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Ok(UploadPrincipal::Device(device_id));
    }
    authorize_shared(headers).map(|()| UploadPrincipal::SharedToken)
}

/// The pre-pairing check, unchanged: no configured token means an open
/// deployment, which `validate_configuration` forbids for remote storage.
pub fn authorize_shared(headers: &HeaderMap) -> Result<(), StatusCode> {
    let Ok(expected) = std::env::var("DAILY_MIRROR_UPLOAD_TOKEN") else {
        return Ok(());
    };
    if expected.is_empty() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    if bearer(headers) == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

pub fn validate_configuration(store: &PhotoStore) -> io::Result<()> {
    if store.is_remote()
        && std::env::var("DAILY_MIRROR_UPLOAD_TOKEN")
            .ok()
            .is_none_or(|token| token.is_empty())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DAILY_MIRROR_UPLOAD_TOKEN must be set when R2 storage is enabled",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::authorize_shared;

    #[test]
    fn bearer_token_is_required_when_configured() {
        // Environment mutation is process-global. Use a unique value and restore it so this
        // test remains friendly to callers running the suite with an existing local token.
        let previous = std::env::var("DAILY_MIRROR_UPLOAD_TOKEN").ok();
        unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", "test-upload-token") };

        let mut headers = HeaderMap::new();
        assert!(authorize_shared(&headers).is_err());
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-upload-token"),
        );
        assert!(authorize_shared(&headers).is_ok());

        match previous {
            Some(value) => unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", value) },
            None => unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") },
        }
    }
}
