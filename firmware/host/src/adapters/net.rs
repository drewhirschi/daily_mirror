//! Net: `reqwest` for the real calls, plus a small local HTTP server standing
//! in for the board's SoftAP provisioning link.
//!
//! On the P4, `network_provisioning` runs a SoftAP the phone joins, and the
//! claim token arrives on a custom provisioning endpoint. There is no SoftAP
//! on a laptop, so `start_provisioning` opens a plain HTTP listener on
//! `127.0.0.1:<port>` with the same shape:
//!
//! | Route | Meaning |
//! | --- | --- |
//! | `GET /` | device id and firmware, what the app's discovery step reads |
//! | `POST /provision` | `{ssid, psk, server_url, claim_token}` — the payload the app sends |
//! | `GET /status` | the device's last `ProvisioningResult` |
//!
//! `join` is a no-op: the host is already on the network, so it reports
//! success and the runtime moves straight to the claim. Everything past that
//! point — the claim and the grant/PUT/complete upload — is the real protocol
//! against a real server.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use daily_mirror_core::contract::{DeviceClaimRequest, DeviceClaimed, ProvisioningResult};
use daily_mirror_core::ports::{Net, QueuedCapture, ReceivedCredentials};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

/// The body the app posts to `/provision`. Wi-Fi credentials travel in the
/// standard provisioning `wifi_config` step on the board; here they share one
/// request because there is only one link.
#[derive(Debug, Deserialize, Serialize)]
struct ProvisionRequest {
    ssid: String,
    psk: String,
    server_url: String,
    claim_token: String,
}

#[derive(Serialize)]
struct UploadGrantRequest<'a> {
    capture_id: &'a str,
    content_type: &'static str,
    content_length: u64,
}

#[derive(Deserialize)]
struct UploadGrant {
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    complete_url: Option<String>,
}

#[derive(Debug, Default)]
struct Shared {
    inbox: Mutex<VecDeque<ReceivedCredentials>>,
    device_id: Mutex<String>,
    last_report: Mutex<Option<ProvisioningResult>>,
}

#[derive(Debug)]
struct ProvisioningServer {
    running: Arc<AtomicBool>,
    port: u16,
}

#[derive(Debug)]
pub struct HttpNet {
    client: Client,
    requested_port: u16,
    shared: Arc<Shared>,
    server: Option<ProvisioningServer>,
}

impl HttpNet {
    pub fn new(provisioning_port: u16) -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .build()
                .expect("build the HTTP client"),
            requested_port: provisioning_port,
            shared: Arc::new(Shared::default()),
            server: None,
        }
    }

    /// The port the provisioning listener actually bound, once started. With a
    /// requested port of 0 the operating system picks one, which is how the
    /// tests avoid fighting over a fixed port.
    pub fn provisioning_port(&self) -> Option<u16> {
        self.server.as_ref().map(|server| server.port)
    }

    /// The device's last report over the provisioning link.
    pub fn last_report(&self) -> Option<ProvisioningResult> {
        self.shared.last_report.lock().ok().and_then(|r| r.clone())
    }
}

impl Drop for HttpNet {
    fn drop(&mut self) {
        if let Some(server) = &self.server {
            server.running.store(false, Ordering::SeqCst);
        }
    }
}

impl Net for HttpNet {
    type Error = anyhow::Error;

    fn start_provisioning(&mut self, device_id: &str) -> Result<()> {
        if let Ok(mut held) = self.shared.device_id.lock() {
            *held = device_id.to_string();
        }
        if self.server.is_some() {
            return Ok(());
        }
        let listener = TcpListener::bind(("127.0.0.1", self.requested_port))
            .with_context(|| format!("bind the provisioning port {}", self.requested_port))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let running = Arc::new(AtomicBool::new(true));
        let shared = Arc::clone(&self.shared);
        let thread_running = Arc::clone(&running);
        thread::spawn(move || {
            while thread_running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        if let Err(error) = serve(stream, &shared) {
                            eprintln!("provisioning request failed: {error:#}");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => {
                        eprintln!("provisioning listener stopped: {error:#}");
                        break;
                    }
                }
            }
        });
        eprintln!("provisioning link on http://127.0.0.1:{port} (POST /provision)");
        self.server = Some(ProvisioningServer { running, port });
        Ok(())
    }

    fn stop_provisioning(&mut self) -> Result<()> {
        if let Some(server) = self.server.take() {
            server.running.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    fn poll_credentials(&mut self) -> Option<ReceivedCredentials> {
        self.shared.inbox.lock().ok()?.pop_front()
    }

    fn report(&mut self, result: &ProvisioningResult) -> Result<()> {
        if let Ok(mut last) = self.shared.last_report.lock() {
            *last = Some(result.clone());
        }
        Ok(())
    }

    fn join(&mut self, ssid: &str, _psk: &str) -> Result<()> {
        // The host is already on a network. The board's adapter calls
        // esp_wifi_remote here and waits for the got-IP event.
        eprintln!("wifi join requested for {ssid:?}; the host is already connected");
        Ok(())
    }

    fn is_connected(&mut self) -> bool {
        true
    }

    fn claim(&mut self, server_url: &str, request: &DeviceClaimRequest) -> Result<DeviceClaimed> {
        let endpoint = join_url(server_url, "/api/devices/claim")?;
        let response = self
            .client
            .post(endpoint)
            .json(request)
            .send()
            .context("redeem the claim token")?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().unwrap_or_default();
            bail!(
                "server rejected the claim with HTTP {status}: {}",
                detail.trim()
            );
        }
        response.json().context("decode the claim response")
    }

    fn upload(
        &mut self,
        server_url: &str,
        device_token: &str,
        capture: &QueuedCapture,
        jpeg: &[u8],
    ) -> Result<()> {
        let grant_endpoint = join_url(server_url, "/api/uploads")?;
        let size = jpeg.len() as u64;

        let response = self
            .client
            .post(grant_endpoint.clone())
            .header(AUTHORIZATION, format!("Bearer {device_token}"))
            .json(&UploadGrantRequest {
                capture_id: &capture.capture_id,
                content_type: "image/jpeg",
                content_length: size,
            })
            .send()
            .context("request a signed upload")?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().unwrap_or_default();
            bail!(
                "server rejected the upload request with HTTP {status}: {}",
                detail.trim()
            );
        }
        let grant: UploadGrant = response.json().context("decode the upload grant")?;

        let target = grant_endpoint
            .join(&grant.url)
            .context("resolve the signed upload target")?;
        let target_is_server = same_origin(&grant_endpoint, &target);
        let method = match grant.method.to_ascii_uppercase().as_str() {
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            other => bail!("server returned unsupported upload method {other:?}"),
        };

        let mut request = self
            .client
            .request(method, target)
            .header(CONTENT_LENGTH, size)
            .body(jpeg.to_vec());
        for (name, value) in grant.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid upload header {name:?}"))?;
            let value = HeaderValue::from_str(&value).context("invalid upload header value")?;
            request = request.header(name, value);
        }
        if target_is_server {
            request = request.header(AUTHORIZATION, format!("Bearer {device_token}"));
        }
        let response = request
            .send()
            .context("send the photo to the upload target")?;
        if !response.status().is_success() {
            bail!(
                "upload target rejected the photo with HTTP {}",
                response.status()
            );
        }

        if let Some(complete_url) = grant.complete_url {
            let endpoint = grant_endpoint
                .join(&complete_url)
                .context("resolve the completion endpoint")?;
            let completion = self
                .client
                .post(endpoint)
                .header(AUTHORIZATION, format!("Bearer {device_token}"))
                .send()
                .context("confirm the completed upload")?;
            if !completion.status().is_success() {
                bail!(
                    "server rejected the upload completion with HTTP {}",
                    completion.status()
                );
            }
        }
        Ok(())
    }
}

fn join_url(server_url: &str, path: &str) -> Result<Url> {
    let base = Url::parse(server_url)
        .with_context(|| format!("invalid Daily Mirror server URL: {server_url}"))?;
    base.join(path)
        .with_context(|| format!("build {path} against {server_url}"))
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

/// Serve one provisioning request. Deliberately the smallest HTTP subset that
/// works: a request line, headers, and an optional `Content-Length` body.
fn serve(stream: TcpStream, shared: &Shared) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    let (status, payload) = route(&method, &path, &body, shared);
    respond(stream, status, &payload)
}

fn route(method: &str, path: &str, body: &[u8], shared: &Shared) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/") => {
            let device_id = shared
                .device_id
                .lock()
                .map(|id| id.clone())
                .unwrap_or_default();
            (
                "200 OK",
                serde_json::json!({
                    "device_id": device_id,
                    "firmware_version": crate::FIRMWARE_VERSION,
                    "hardware": crate::HARDWARE,
                })
                .to_string(),
            )
        }
        ("GET", "/status") => {
            let report = shared.last_report.lock().ok().and_then(|r| r.clone());
            match report {
                Some(report) => (
                    "200 OK",
                    serde_json::to_string(&report).unwrap_or_else(|_| "{}".into()),
                ),
                None => ("200 OK", r#"{"status":"pairing"}"#.to_string()),
            }
        }
        ("POST", "/provision") => match serde_json::from_slice::<ProvisionRequest>(body) {
            Ok(request) => {
                if let Ok(mut inbox) = shared.inbox.lock() {
                    inbox.push_back(ReceivedCredentials {
                        ssid: request.ssid,
                        psk: request.psk,
                        server_url: request.server_url,
                        claim_token: request.claim_token,
                    });
                }
                ("202 Accepted", r#"{"status":"accepted"}"#.to_string())
            }
            Err(error) => (
                "400 Bad Request",
                serde_json::json!({ "error": error.to_string() }).to_string(),
            ),
        },
        _ => (
            "404 Not Found",
            r#"{"error":"no such endpoint"}"#.to_string(),
        ),
    }
}

fn respond(mut stream: TcpStream, status: &str, payload: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Post credentials to a running provisioning link. Used by the tests and by
/// anyone driving a live host run from another terminal.
pub fn post_credentials(port: u16, credentials: &ReceivedCredentials) -> Result<()> {
    let body = serde_json::to_string(&ProvisionRequest {
        ssid: credentials.ssid.clone(),
        psk: credentials.psk.clone(),
        server_url: credentials.server_url.clone(),
        claim_token: credentials.claim_token.clone(),
    })?;
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connect to the provisioning link on port {port}"))?;
    stream.write_all(
        format!(
            "POST /provision HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )?;
    stream.flush()?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.1 202") {
        return Err(anyhow!(
            "provisioning link refused the credentials: {response}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> ReceivedCredentials {
        ReceivedCredentials {
            ssid: "home".into(),
            psk: "secret".into(),
            server_url: "https://mirror.example".into(),
            claim_token: "ct_1".into(),
        }
    }

    #[test]
    fn the_provisioning_link_accepts_credentials_over_http() {
        let mut net = HttpNet::new(0);
        net.start_provisioning("dm-test").unwrap();
        let port = net.provisioning_port().expect("a bound port");

        assert!(net.poll_credentials().is_none());
        post_credentials(port, &credentials()).unwrap();

        let mut received = None;
        for _ in 0..200 {
            if let Some(value) = net.poll_credentials() {
                received = Some(value);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(received, Some(credentials()));
        net.stop_provisioning().unwrap();
    }

    #[test]
    fn reports_are_remembered_for_the_app_to_read() {
        let mut net = HttpNet::new(0);
        assert!(net.last_report().is_none());
        net.report(&ProvisioningResult::AwaitingConfirm).unwrap();
        assert_eq!(net.last_report(), Some(ProvisioningResult::AwaitingConfirm));
    }

    #[test]
    fn unknown_routes_are_not_found() {
        let shared = Shared::default();
        let (status, _) = route("GET", "/nope", b"", &shared);
        assert_eq!(status, "404 Not Found");
        let (status, _) = route("POST", "/provision", b"not json", &shared);
        assert_eq!(status, "400 Bad Request");
    }

    #[test]
    fn same_origin_compares_scheme_host_and_port() {
        let base = Url::parse("http://127.0.0.1:3000/api/uploads").unwrap();
        assert!(same_origin(&base, &base.join("/api/uploads/abc").unwrap()));
        assert!(!same_origin(
            &base,
            &Url::parse("https://storage.example/abc").unwrap()
        ));
    }
}
