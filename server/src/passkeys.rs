use std::io;

use serde::Serialize;
use webauthn_rs::prelude::{
    CreationChallengeResponse, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse, Url, Webauthn, WebauthnBuilder,
};

use crate::auth::{AuthStore, User};

const REGISTRATION_KIND: &str = "passkey-registration";
const AUTHENTICATION_KIND: &str = "passkey-authentication";

#[derive(Clone, Debug)]
pub struct PasskeyService {
    webauthn: Webauthn,
    secure_cookies: bool,
}

#[derive(Debug, Serialize)]
pub struct PasskeyStart<T> {
    pub ceremony_id: String,
    pub options: T,
}

impl PasskeyService {
    pub fn from_env() -> io::Result<Self> {
        let origin = configured_origin()?;
        let rp_id = std::env::var("DAILY_MIRROR_AUTH_RP_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Self::new(&origin, rp_id)
    }

    fn new(origin: &str, rp_id: Option<String>) -> io::Result<Self> {
        let url = Url::parse(origin).map_err(invalid_config)?;
        let rp_id = rp_id
            .or_else(|| url.host_str().map(str::to_owned))
            .ok_or_else(|| invalid_config("authentication origin must include a host"))?;
        let mut builder = WebauthnBuilder::new(&rp_id, &url).map_err(invalid_config)?;
        builder = builder.rp_name("Daily Mirror");
        let webauthn = builder.build().map_err(invalid_config)?;
        Ok(Self {
            webauthn,
            secure_cookies: url.scheme() == "https",
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.secure_cookies
    }

    pub async fn start_registration(
        &self,
        store: &AuthStore,
        user: &User,
    ) -> io::Result<PasskeyStart<CreationChallengeResponse>> {
        let existing = store.passkeys(&user.id).await?;
        let exclude = (!existing.is_empty()).then(|| {
            existing
                .iter()
                .map(|stored| stored.passkey.cred_id().clone())
                .collect()
        });
        let (options, state) = self
            .webauthn
            .start_passkey_registration(user.uuid()?, &user.username, &user.display_name, exclude)
            .map_err(authentication_failed)?;
        let state_json = serde_json::to_string(&state).map_err(invalid_data)?;
        let ceremony_id = store
            .store_ceremony(REGISTRATION_KIND, &user.id, &state_json)
            .await?;
        Ok(PasskeyStart {
            ceremony_id,
            options,
        })
    }

    pub async fn finish_registration(
        &self,
        store: &AuthStore,
        user: &User,
        ceremony_id: &str,
        label: &str,
        credential: &RegisterPublicKeyCredential,
    ) -> io::Result<()> {
        let ceremony = store
            .take_ceremony(ceremony_id, REGISTRATION_KIND)
            .await?
            .ok_or_else(|| authentication_failed("registration ceremony expired or was used"))?;
        if ceremony.user_id != user.id {
            return Err(authentication_failed(
                "registration ceremony belongs to another account",
            ));
        }
        let state: PasskeyRegistration =
            serde_json::from_str(&ceremony.state_json).map_err(invalid_data)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &state)
            .map_err(authentication_failed)?;
        store.add_passkey(&user.id, label, &passkey).await
    }

    pub async fn start_authentication(
        &self,
        store: &AuthStore,
        user: &User,
    ) -> io::Result<PasskeyStart<RequestChallengeResponse>> {
        let passkeys = store.passkeys(&user.id).await?;
        if passkeys.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "this account has no passkeys",
            ));
        }
        let credentials = passkeys
            .iter()
            .map(|stored| stored.passkey.clone())
            .collect::<Vec<_>>();
        let (options, state) = self
            .webauthn
            .start_passkey_authentication(&credentials)
            .map_err(authentication_failed)?;
        let state_json = serde_json::to_string(&state).map_err(invalid_data)?;
        let ceremony_id = store
            .store_ceremony(AUTHENTICATION_KIND, &user.id, &state_json)
            .await?;
        Ok(PasskeyStart {
            ceremony_id,
            options,
        })
    }

    pub async fn finish_authentication(
        &self,
        store: &AuthStore,
        ceremony_id: &str,
        credential: &PublicKeyCredential,
    ) -> io::Result<User> {
        let ceremony = store
            .take_ceremony(ceremony_id, AUTHENTICATION_KIND)
            .await?
            .ok_or_else(|| authentication_failed("authentication ceremony expired or was used"))?;
        let state: PasskeyAuthentication =
            serde_json::from_str(&ceremony.state_json).map_err(invalid_data)?;
        let result = self
            .webauthn
            .finish_passkey_authentication(credential, &state)
            .map_err(authentication_failed)?;
        let mut passkeys = store.passkeys(&ceremony.user_id).await?;
        let mut authenticated = None;
        for stored in &mut passkeys {
            if stored.passkey.update_credential(&result).is_some() {
                authenticated = Some(stored);
                break;
            }
        }
        let stored = authenticated
            .ok_or_else(|| authentication_failed("authenticated passkey is not registered"))?;
        store.update_passkey(&ceremony.user_id, stored).await?;
        store
            .user_by_id(&ceremony.user_id)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "account no longer exists"))
    }
}

fn configured_origin() -> io::Result<String> {
    if std::env::var("VERCEL_ENV").as_deref() == Ok("preview")
        && let Ok(host) = std::env::var("VERCEL_URL")
    {
        return Ok(format!("https://{host}"));
    }
    match std::env::var("DAILY_MIRROR_AUTH_ORIGIN") {
        Ok(origin) if !origin.trim().is_empty() => Ok(origin),
        _ if std::env::var("VERCEL_ENV").as_deref() == Ok("production") => Err(invalid_config(
            "DAILY_MIRROR_AUTH_ORIGIN is required in Vercel production",
        )),
        _ => Ok("http://localhost:3000".to_owned()),
    }
}

fn invalid_config(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn authentication_failed(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{PasskeyRegistration, PasskeyService, REGISTRATION_KIND};
    use crate::auth::AuthStore;

    async fn fixture(name: &str) -> (std::path::PathBuf, AuthStore) {
        let root = std::env::temp_dir().join(format!(
            "daily-mirror-passkeys-{name}-{}",
            std::process::id()
        ));
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&root).await.unwrap();
        let store = AuthStore::local(root.join("auth.db").to_string_lossy().into_owned());
        (root, store)
    }

    #[test]
    fn cookie_security_follows_the_canonical_origin() {
        assert!(
            PasskeyService::new("https://mirror.example", None)
                .unwrap()
                .secure_cookies()
        );
        assert!(
            !PasskeyService::new("http://localhost:3000", None)
                .unwrap()
                .secure_cookies()
        );
        assert!(PasskeyService::new("not a URL", None).is_err());
    }

    #[tokio::test]
    async fn registration_start_persists_single_use_server_state() {
        let (root, store) = fixture("registration").await;
        let user = store
            .create_user("drew", "Drew", "a-good-test-password")
            .await
            .unwrap();
        let service = PasskeyService::new("https://mirror.example", None).unwrap();

        let start = service.start_registration(&store, &user).await.unwrap();
        let options = serde_json::to_value(&start.options).unwrap();
        assert!(options.pointer("/publicKey/challenge").is_some());
        assert_eq!(
            options
                .pointer("/publicKey/rp/id")
                .and_then(|value| value.as_str()),
            Some("mirror.example")
        );

        let ceremony = store
            .take_ceremony(&start.ceremony_id, REGISTRATION_KIND)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ceremony.user_id, user.id);
        let _: PasskeyRegistration = serde_json::from_str(&ceremony.state_json).unwrap();
        assert!(
            store
                .take_ceremony(&start.ceremony_id, REGISTRATION_KIND)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            service
                .start_authentication(&store, &user)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );

        drop(store);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
