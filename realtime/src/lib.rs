mod protocol;

use protocol::{EventEnvelope, IncomingEvent, valid_identifier, validate_event, verify_ticket};
use serde_json::json;
use worker::*;

const MAX_EVENT_BYTES: usize = 64 * 1024;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let path = req.path();
    if req.method() == Method::Get && path == "/healthz" {
        return Response::from_json(&json!({
            "status": "ok",
            "service": "daily-mirror-realtime",
            "version": env!("CARGO_PKG_VERSION")
        }));
    }

    let Some((household_id, action)) = household_route(&path) else {
        return Response::error("not found", 404);
    };
    if !valid_identifier(household_id) {
        return Response::error("invalid household", 400);
    }

    match (req.method(), action) {
        (Method::Get, "connect") => {
            if let Some(response) = authorize_connection(&req, &env, household_id)? {
                return Ok(response);
            }
        }
        (Method::Post, "events") => {
            if let Some(response) = authorize_publisher(&req, &env)? {
                return Ok(response);
            }
        }
        _ => return Response::error("method not allowed", 405),
    }

    let namespace = env.durable_object("HOUSEHOLDS")?;
    let stub = namespace.id_from_name(household_id)?.get_stub()?;
    stub.fetch_with_request(req).await
}

fn authorize_connection(req: &Request, env: &Env, household_id: &str) -> Result<Option<Response>> {
    if req.headers().get("upgrade")?.as_deref() != Some("websocket") {
        return Response::error("websocket upgrade required", 426).map(Some);
    }
    let origin = req.headers().get("origin")?.unwrap_or_default();
    let allowed_origins = env.var("ALLOWED_ORIGINS")?.to_string();
    if !origin_allowed(&origin, &allowed_origins) {
        return Response::error("origin not allowed", 403).map(Some);
    }
    let url = req.url()?;
    let ticket = url
        .query_pairs()
        .find_map(|(key, value)| (key == "ticket").then_some(value));
    let Some(ticket) = ticket else {
        return Response::error("ticket required", 401).map(Some);
    };
    let now = (Date::now().as_millis() / 1_000) as i64;
    if verify_ticket(
        &ticket,
        &secret_or_local_var(env, "TICKET_SECRET")?,
        household_id,
        now,
    )
    .is_err()
    {
        return Response::error("invalid or expired ticket", 401).map(Some);
    }
    Ok(None)
}

fn authorize_publisher(req: &Request, env: &Env) -> Result<Option<Response>> {
    let expected = format!("Bearer {}", secret_or_local_var(env, "PUBLISH_TOKEN")?);
    let actual = req.headers().get("authorization")?.unwrap_or_default();
    if actual != expected {
        return Response::error("publisher unauthorized", 401).map(Some);
    }
    Ok(None)
}

fn secret_or_local_var(env: &Env, name: &str) -> Result<String> {
    env.secret(name)
        .map(|value| value.to_string())
        .or_else(|_| env.var(name).map(|value| value.to_string()))
}

fn household_route(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/v1/households/")?;
    let (household, action) = rest.split_once('/')?;
    (!household.is_empty() && !action.contains('/')).then_some((household, action))
}

fn origin_allowed(origin: &str, configured: &str) -> bool {
    configured
        .split(',')
        .map(str::trim)
        .any(|allowed| !allowed.is_empty() && allowed == origin)
}

#[durable_object]
pub struct HouseholdRealtime {
    state: State,
}

impl DurableObject for HouseholdRealtime {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let path = req.path();
        let (household_id, action) = household_route(&path)
            .ok_or_else(|| Error::RustError("invalid household route".into()))?;
        self.remember_household(household_id).await?;

        if req.method() == Method::Get && action == "connect" {
            return self.connect();
        }
        if req.method() == Method::Post && action == "events" {
            return self.publish(household_id, &mut req).await;
        }
        Response::error("not found", 404)
    }

    async fn websocket_message(
        &self,
        websocket: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message
            && text == "ping"
        {
            websocket.send_with_str("pong")?;
        }
        Ok(())
    }

    async fn websocket_close(
        &self,
        websocket: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        websocket.close(Some(code as u16), Some(reason))
    }

    async fn websocket_error(&self, websocket: WebSocket, _error: Error) -> Result<()> {
        websocket.close(Some(1011), Some("realtime connection failed"))
    }
}

impl HouseholdRealtime {
    fn connect(&self) -> Result<Response> {
        let pair = WebSocketPair::new()?;
        let client = pair.client;
        let server = pair.server;
        self.state.accept_web_socket(&server);
        Response::from_websocket(client)
    }

    async fn publish(&self, household_id: &str, req: &mut Request) -> Result<Response> {
        let body = req.bytes().await?;
        if body.len() > MAX_EVENT_BYTES {
            return Response::error("event too large", 413);
        }
        let event: IncomingEvent = serde_json::from_slice(&body)?;
        if let Err(error) = validate_event(&event) {
            return Response::error(error, 400);
        }

        let storage = self.state.storage();
        let sequence = storage.get::<u64>("sequence").await?.unwrap_or(0) + 1;
        storage.put("sequence", sequence).await?;
        let envelope = EventEnvelope {
            kind: event.kind,
            household_id: household_id.into(),
            sequence,
            occurred_at: Date::now().to_string(),
            data: event.data,
        };
        let message = serde_json::to_string(&envelope)?;
        let mut delivered = 0;
        for websocket in self.state.get_websockets() {
            if websocket.send_with_str(&message).is_ok() {
                delivered += 1;
            }
        }
        Response::from_json(&json!({ "sequence": sequence, "delivered": delivered }))
    }

    async fn remember_household(&self, household_id: &str) -> Result<()> {
        let storage = self.state.storage();
        let existing = storage.get::<String>("household_id").await?;
        if let Some(existing) = &existing
            && existing != household_id
        {
            return Err(Error::RustError("household identity mismatch".into()));
        }
        if existing.is_none() {
            storage.put("household_id", household_id).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_exact_household_routes() {
        assert_eq!(
            household_route("/v1/households/home-1/connect"),
            Some(("home-1", "connect"))
        );
        assert_eq!(
            household_route("/v1/households/home-1/events"),
            Some(("home-1", "events"))
        );
        assert_eq!(household_route("/v1/households/home-1/events/extra"), None);
        assert_eq!(household_route("/healthz"), None);
    }

    #[test]
    fn matches_origins_as_exact_list_entries() {
        assert!(origin_allowed(
            "https://mirror.example",
            "http://localhost:3000, https://mirror.example"
        ));
        assert!(!origin_allowed(
            "https://evil.example",
            "https://mirror.example"
        ));
        assert!(!origin_allowed(
            "https://mirror.example.evil",
            "https://mirror.example"
        ));
    }
}
