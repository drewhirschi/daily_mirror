use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use utoipa::ToSchema;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const TICKET_LIFETIME_SECONDS: i64 = 60;

#[derive(Clone)]
pub struct RealtimeHub {
    config: Option<RealtimeConfig>,
    client: reqwest::Client,
}

#[derive(Clone)]
struct RealtimeConfig {
    base_url: String,
    household_id: String,
    ticket_secret: String,
    publish_token: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct RealtimeSession {
    pub household_id: String,
    pub websocket_url: String,
    pub expires_at: i64,
}

#[derive(Serialize)]
struct TicketClaims<'a> {
    household_id: &'a str,
    expires_at: i64,
    nonce: String,
}

#[derive(Serialize)]
struct OutgoingEvent<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    data: Value,
}

impl RealtimeHub {
    pub fn from_env() -> Result<Self, String> {
        let Some(base_url) = optional_env("DAILY_MIRROR_REALTIME_URL") else {
            return Ok(Self::disabled());
        };
        let household_id = required_env("DAILY_MIRROR_HOUSEHOLD_ID")?;
        if !valid_identifier(&household_id) {
            return Err(
                "DAILY_MIRROR_HOUSEHOLD_ID may contain only letters, numbers, '-' and '_'".into(),
            );
        }
        let ticket_secret = required_secret("DAILY_MIRROR_REALTIME_TICKET_SECRET")?;
        let publish_token = required_secret("DAILY_MIRROR_REALTIME_PUBLISH_TOKEN")?;
        Self::configured(base_url, household_id, ticket_secret, publish_token)
    }

    fn disabled() -> Self {
        Self {
            config: None,
            client: reqwest::Client::new(),
        }
    }

    fn configured(
        base_url: String,
        household_id: String,
        ticket_secret: String,
        publish_token: String,
    ) -> Result<Self, String> {
        let base_url = base_url.trim_end_matches('/').to_owned();
        let parsed = reqwest::Url::parse(&base_url)
            .map_err(|error| format!("DAILY_MIRROR_REALTIME_URL is invalid: {error}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("DAILY_MIRROR_REALTIME_URL must use http or https".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|error| format!("could not construct realtime client: {error}"))?;
        Ok(Self {
            config: Some(RealtimeConfig {
                base_url,
                household_id,
                ticket_secret,
                publish_token,
            }),
            client,
        })
    }

    pub fn session(&self) -> Result<Option<RealtimeSession>, String> {
        let Some(config) = &self.config else {
            return Ok(None);
        };
        let now = unix_seconds()?;
        let expires_at = now + TICKET_LIFETIME_SECONDS;
        let claims = TicketClaims {
            household_id: &config.household_id,
            expires_at,
            nonce: Uuid::new_v4().simple().to_string(),
        };
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&claims)
                .map_err(|error| format!("could not encode realtime ticket: {error}"))?,
        );
        let mut mac = HmacSha256::new_from_slice(config.ticket_secret.as_bytes())
            .map_err(|_| "invalid realtime ticket secret".to_owned())?;
        mac.update(payload.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        let websocket_base = config
            .base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        Ok(Some(RealtimeSession {
            household_id: config.household_id.clone(),
            websocket_url: format!(
                "{websocket_base}/v1/households/{}/connect?ticket={payload}.{signature}",
                config.household_id
            ),
            expires_at,
        }))
    }

    pub async fn publish_photo(&self, kind: &'static str, photo_id: &str) {
        self.publish(kind, json!({ "photo_id": photo_id })).await;
    }

    pub async fn publish_reconciled(&self) {
        self.publish("photos.reconciled", json!({})).await;
    }

    async fn publish(&self, kind: &'static str, data: Value) {
        let Some(config) = &self.config else { return };
        let url = format!(
            "{}/v1/households/{}/events",
            config.base_url, config.household_id
        );
        let result = self
            .client
            .post(url)
            .bearer_auth(&config.publish_token)
            .json(&OutgoingEvent { kind, data })
            .send()
            .await;
        if let Err(error) = result.and_then(reqwest::Response::error_for_status) {
            eprintln!("realtime event delivery failed: {error}");
        }
    }
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn required_env(name: &str) -> Result<String, String> {
    optional_env(name)
        .ok_or_else(|| format!("{name} is required when DAILY_MIRROR_REALTIME_URL is set"))
}

fn required_secret(name: &str) -> Result<String, String> {
    let value = required_env(name)?;
    if value.len() < 32 {
        return Err(format!("{name} must be at least 32 characters"));
    }
    Ok(value)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn unix_seconds() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub() -> RealtimeHub {
        RealtimeHub::configured(
            "https://realtime.example/".into(),
            "home-1".into(),
            "t".repeat(32),
            "p".repeat(32),
        )
        .unwrap()
    }

    #[test]
    fn disabled_hub_has_no_session() {
        assert!(RealtimeHub::disabled().session().unwrap().is_none());
    }

    #[test]
    fn creates_a_short_lived_signed_websocket_session() {
        let session = hub().session().unwrap().unwrap();
        assert_eq!(session.household_id, "home-1");
        assert!(
            session
                .websocket_url
                .starts_with("wss://realtime.example/v1/households/home-1/connect?ticket=")
        );
        assert!(session.expires_at - unix_seconds().unwrap() <= TICKET_LIFETIME_SECONDS);

        let ticket = session.websocket_url.split("ticket=").nth(1).unwrap();
        let (payload, signature) = ticket.split_once('.').unwrap();
        let mut mac = HmacSha256::new_from_slice("t".repeat(32).as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        mac.verify_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap())
            .unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
        assert_eq!(claims["household_id"], "home-1");
    }

    #[test]
    fn validates_configuration() {
        assert!(
            RealtimeHub::configured(
                "ftp://example.com".into(),
                "home".into(),
                "t".repeat(32),
                "p".repeat(32)
            )
            .is_err()
        );
        assert!(!valid_identifier("../../home"));
    }
}
