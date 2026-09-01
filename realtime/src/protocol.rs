use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const MAX_CLOCK_SKEW_SECONDS: i64 = 10;
const MAX_EVENT_TYPE_BYTES: usize = 80;
const MAX_IDENTIFIER_BYTES: usize = 120;

#[derive(Debug, Deserialize, Serialize)]
pub struct TicketClaims {
    pub household_id: String,
    pub expires_at: i64,
    pub nonce: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IncomingEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    pub household_id: String,
    pub sequence: u64,
    pub occurred_at: String,
    pub data: serde_json::Value,
}

pub fn verify_ticket(
    ticket: &str,
    secret: &str,
    expected_household: &str,
    now_unix_seconds: i64,
) -> Result<TicketClaims, &'static str> {
    let (payload, signature) = ticket.split_once('.').ok_or("invalid ticket")?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| "invalid ticket")?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| "invalid secret")?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).map_err(|_| "invalid ticket")?;

    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "invalid ticket")?;
    let claims: TicketClaims = serde_json::from_slice(&payload).map_err(|_| "invalid ticket")?;
    if claims.household_id != expected_household {
        return Err("ticket belongs to another household");
    }
    if claims.expires_at + MAX_CLOCK_SKEW_SECONDS < now_unix_seconds {
        return Err("ticket expired");
    }
    if !valid_identifier(&claims.household_id) || !valid_identifier(&claims.nonce) {
        return Err("invalid ticket");
    }
    Ok(claims)
}

pub fn validate_event(event: &IncomingEvent) -> Result<(), &'static str> {
    let kind = event.kind.as_bytes();
    if kind.is_empty() || kind.len() > MAX_EVENT_TYPE_BYTES {
        return Err("invalid event type");
    }
    if !event
        .kind
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return Err("invalid event type");
    }
    Ok(())
}

pub fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(claims: &TicketClaims, secret: &str) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{payload}.{signature}")
    }

    fn claims() -> TicketClaims {
        TicketClaims {
            household_id: "home-1".into(),
            expires_at: 1_000,
            nonce: "nonce-1".into(),
        }
    }

    #[test]
    fn accepts_a_valid_household_ticket() {
        let verified =
            verify_ticket(&ticket(&claims(), "secret"), "secret", "home-1", 999).unwrap();
        assert_eq!(verified.household_id, "home-1");
    }

    #[test]
    fn rejects_tampered_expired_and_cross_household_tickets() {
        let signed = ticket(&claims(), "secret");
        assert!(verify_ticket(&format!("{signed}x"), "secret", "home-1", 999).is_err());
        assert!(verify_ticket(&signed, "secret", "home-1", 1_011).is_err());
        assert!(verify_ticket(&signed, "secret", "home-2", 999).is_err());
    }

    #[test]
    fn validates_event_types_and_identifiers() {
        assert!(
            validate_event(&IncomingEvent {
                kind: "photo.created".into(),
                data: serde_json::Value::Null
            })
            .is_ok()
        );
        assert!(
            validate_event(&IncomingEvent {
                kind: "bad event".into(),
                data: serde_json::Value::Null
            })
            .is_err()
        );
        assert!(valid_identifier("household_01"));
        assert!(!valid_identifier("../../household"));
    }
}
