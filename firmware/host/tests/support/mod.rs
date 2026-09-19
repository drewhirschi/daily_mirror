//! A mock Daily Mirror server: the two pairing routes and the grant / PUT /
//! complete upload sequence, on `std::net` so the test suite needs no HTTP
//! server dependency.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// A tiny but structurally valid JPEG: SOI, a byte, EOI.
pub const FIXTURE_JPEG: &[u8] = &[0xff, 0xd8, 0x42, 0xff, 0xd9];

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique temporary directory for one test.
pub fn temp_dir(label: &str) -> PathBuf {
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "daily-mirror-{label}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Write the fixture JPEG the camera adapter returns.
pub fn write_fixture() -> PathBuf {
    let path = temp_dir("fixture").join("feed.jpg");
    std::fs::write(&path, FIXTURE_JPEG).unwrap();
    path
}

#[derive(Clone, Debug)]
pub struct RecordedClaim {
    pub device_id: String,
    pub claim_token: String,
    pub firmware_version: String,
    pub hardware: String,
}

#[derive(Clone, Debug)]
pub struct RecordedUpload {
    pub capture_id: String,
    pub bearer: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Default)]
struct State {
    claims: Mutex<Vec<RecordedClaim>>,
    uploads: Mutex<Vec<RecordedUpload>>,
    completed: AtomicBool,
    reject_claims: AtomicBool,
    fail_uploads: AtomicBool,
}

pub struct MockServer {
    port: u16,
    state: Arc<State>,
    running: Arc<AtomicBool>,
}

impl MockServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(State::default());
        let running = Arc::new(AtomicBool::new(true));

        let thread_state = Arc::clone(&state);
        let thread_running = Arc::clone(&running);
        thread::spawn(move || {
            while thread_running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let state = Arc::clone(&thread_state);
                        thread::spawn(move || {
                            let _ = handle(stream, &state);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            port,
            state,
            running,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn claims(&self) -> Vec<RecordedClaim> {
        self.state.claims.lock().unwrap().clone()
    }

    pub fn uploads(&self) -> Vec<RecordedUpload> {
        self.state.uploads.lock().unwrap().clone()
    }

    pub fn completed(&self) -> bool {
        self.state.completed.load(Ordering::SeqCst)
    }

    pub fn reject_claims(&self) {
        self.state.reject_claims.store(true, Ordering::SeqCst);
    }

    pub fn fail_uploads(&self) {
        self.state.fail_uploads.store(true, Ordering::SeqCst);
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn handle(mut stream: TcpStream, state: &State) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);

    let (status, payload) = route(&method, &path, &body, bearer, state);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn route(
    method: &str,
    path: &str,
    body: &[u8],
    bearer: Option<String>,
    state: &State,
) -> (&'static str, String) {
    match (method, path) {
        ("POST", "/api/devices/claim") => {
            let request: serde_json::Value = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return ("400 Bad Request", r#"{"error":"bad body"}"#.into()),
            };
            state.claims.lock().unwrap().push(RecordedClaim {
                device_id: string_at(&request, "device_id"),
                claim_token: string_at(&request, "claim_token"),
                firmware_version: string_at(&request, "firmware_version"),
                hardware: string_at(&request, "hardware"),
            });
            if state.reject_claims.load(Ordering::SeqCst) {
                return ("403 Forbidden", r#"{"error":"claim token expired"}"#.into());
            }
            (
                "200 OK",
                serde_json::json!({
                    "device_token": "dt_mock",
                    "household_id": "hh_mock",
                    "device_name": "Mock Mirror",
                })
                .to_string(),
            )
        }
        ("POST", "/api/uploads") => {
            if state.fail_uploads.load(Ordering::SeqCst) {
                return ("503 Service Unavailable", r#"{"error":"try later"}"#.into());
            }
            let request: serde_json::Value = match serde_json::from_slice(body) {
                Ok(value) => value,
                Err(_) => return ("400 Bad Request", r#"{"error":"bad body"}"#.into()),
            };
            let capture_id = string_at(&request, "capture_id");
            (
                "200 OK",
                serde_json::json!({
                    "method": "PUT",
                    "url": format!("/api/uploads/{capture_id}"),
                    "headers": { "x-daily-mirror-capture": capture_id },
                    "complete_url": format!("/api/uploads/{capture_id}/complete"),
                })
                .to_string(),
            )
        }
        ("PUT", path) if path.starts_with("/api/uploads/") => {
            let capture_id = path.trim_start_matches("/api/uploads/").to_string();
            state.uploads.lock().unwrap().push(RecordedUpload {
                capture_id,
                bearer,
                body: body.to_vec(),
            });
            ("200 OK", "{}".into())
        }
        ("POST", path) if path.ends_with("/complete") => {
            state.completed.store(true, Ordering::SeqCst);
            ("200 OK", "{}".into())
        }
        _ => ("404 Not Found", r#"{"error":"no such route"}"#.into()),
    }
}

fn string_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}
