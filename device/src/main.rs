use std::collections::BTreeMap;
use std::convert::Infallible;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use rppal::gpio::{Gpio, InputPin, OutputPin};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

mod lab;

use lab::{CameraLab, CameraOrientation, LabSettings, load_orientation};

const RETRY_INTERVAL: Duration = Duration::from_secs(60);
const BUTTON_POLL_INTERVAL: Duration = Duration::from_millis(20);
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(60);

#[derive(Parser)]
#[command(about = "Raspberry Pi camera, button, LED, and upload service for Daily Mirror")]
struct Cli {
    #[command(subcommand)]
    command: DeviceCommand,
}

#[derive(Subcommand)]
enum DeviceCommand {
    /// Take one photograph. The durable queue retains it if upload fails.
    CaptureOnce {
        /// Keep the captured JPEG locally without attempting an upload.
        #[arg(long)]
        no_upload: bool,
    },
    /// Queue and upload an existing JPEG. Useful before GPIO is wired.
    Upload { path: PathBuf },
    /// Retry every JPEG already in the durable queue.
    Retry,
    /// Run continuously with the physical button and three status LEDs.
    Run,
}

#[derive(Clone, Debug)]
struct Config {
    server_url: Option<String>,
    upload_token: Option<String>,
    queue_dir: PathBuf,
    camera_command: String,
    camera_args: Vec<String>,
    camera_settings_path: PathBuf,
    admin_bind: String,
    button_pin: u8,
    green_led_pin: u8,
    yellow_led_pin: u8,
    red_led_pin: u8,
}

impl Config {
    fn from_env() -> Result<Self> {
        let camera_command = std::env::var("DAILY_MIRROR_CAMERA_COMMAND")
            .unwrap_or_else(|_| detect_camera_command());
        let camera_args = std::env::var("DAILY_MIRROR_CAMERA_ARGS")
            .ok()
            .map(|value| shlex::split(&value).ok_or_else(|| anyhow!("invalid camera arguments")))
            .transpose()?
            .unwrap_or_else(default_camera_args);

        Ok(Self {
            server_url: std::env::var("DAILY_MIRROR_SERVER_URL").ok(),
            upload_token: std::env::var("DAILY_MIRROR_UPLOAD_TOKEN").ok(),
            queue_dir: std::env::var_os("DAILY_MIRROR_QUEUE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("data/pending")),
            camera_command,
            camera_args,
            camera_settings_path: std::env::var_os("DAILY_MIRROR_CAMERA_SETTINGS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("data/camera-settings.json")),
            admin_bind: std::env::var("DAILY_MIRROR_ADMIN_BIND")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_owned()),
            button_pin: env_pin("DAILY_MIRROR_BUTTON_PIN", 2)?,
            green_led_pin: env_pin("DAILY_MIRROR_GREEN_LED_PIN", 17)?,
            yellow_led_pin: env_pin("DAILY_MIRROR_YELLOW_LED_PIN", 27)?,
            red_led_pin: env_pin("DAILY_MIRROR_RED_LED_PIN", 22)?,
        })
    }

    fn upload_url(&self) -> Result<String> {
        let base = self
            .server_url
            .as_deref()
            .ok_or_else(|| anyhow!("DAILY_MIRROR_SERVER_URL is required for uploads"))?;
        Ok(format!("{}/api/uploads", base.trim_end_matches('/')))
    }
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let config = Config::from_env()?;
    let device = Device::new(config)?;

    match cli.command {
        DeviceCommand::CaptureOnce { no_upload } => {
            let capture = device.camera.capture()?;
            println!("captured {}", capture.display());
            if !no_upload {
                match device.uploader.upload(&capture) {
                    Ok(()) => println!("uploaded {}", capture.display()),
                    Err(error) => {
                        eprintln!("upload failed; retained for retry: {error:#}");
                    }
                }
            }
        }
        DeviceCommand::Upload { path } => {
            let queued = device.camera.queue_existing(&path)?;
            device.uploader.upload(&queued)?;
            println!("uploaded {}", path.display());
        }
        DeviceCommand::Retry => {
            let remaining = device.uploader.retry_all()?;
            println!("retry complete; {remaining} photo(s) remain queued");
        }
        DeviceCommand::Run => device.run()?,
    }
    Ok(())
}

struct Device {
    config: Config,
    camera: Camera,
    uploader: Uploader,
    operation_lock: Arc<Mutex<()>>,
    runtime_status: watch::Sender<RuntimeStatus>,
    started_at: Instant,
}

impl Device {
    fn new(config: Config) -> Result<Self> {
        let camera = Camera::new(&config)?;
        let uploader = Uploader::new(&config)?;
        let (runtime_status, _) = watch::channel(RuntimeStatus::default());
        Ok(Self {
            config,
            camera,
            uploader,
            operation_lock: Arc::new(Mutex::new(())),
            runtime_status,
            started_at: Instant::now(),
        })
    }

    fn run(&self) -> Result<()> {
        let gpio = Gpio::new().context("initialize Raspberry Pi GPIO")?;
        let button = gpio
            .get(self.config.button_pin)
            .with_context(|| format!("open button GPIO {}", self.config.button_pin))?
            .into_input_pullup();
        let leds = Arc::new(Mutex::new(LedPanel::new(
            &gpio,
            &self.config,
            self.runtime_status.clone(),
        )?));
        let lab = CameraLab::new(
            Arc::clone(&self.camera.camera_lock),
            Arc::clone(&self.camera.orientation),
            self.camera.orientation_path.clone(),
        );
        let led_test_active = Arc::new(AtomicBool::new(false));
        self.start_admin_server(Arc::clone(&leds), Arc::clone(&led_test_active), lab.clone())?;
        let mut last_retry = Instant::now() - RETRY_INTERVAL;

        println!(
            "ready: button BCM {}, LEDs green/yellow/red BCM {}/{}/{}",
            self.config.button_pin,
            self.config.green_led_pin,
            self.config.yellow_led_pin,
            self.config.red_led_pin
        );

        loop {
            if last_retry.elapsed() >= RETRY_INTERVAL {
                let _operation = self
                    .operation_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let retry_result = self.uploader.retry_all();
                let test_active = led_test_active.load(Ordering::Relaxed);
                let remaining = match retry_result {
                    Ok(remaining) => {
                        if !test_active {
                            let message = if remaining == 0 {
                                "Ready".to_owned()
                            } else {
                                format!("Ready; {remaining} photo(s) waiting to upload")
                            };
                            set_runtime_status(&self.runtime_status, "ready", message);
                        }
                        remaining
                    }
                    Err(error) => {
                        eprintln!("queued upload retry failed: {error:#}");
                        if !test_active {
                            set_runtime_status(
                                &self.runtime_status,
                                "error",
                                format!("queued upload retry failed: {error:#}"),
                            );
                        }
                        1
                    }
                };
                if !test_active {
                    leds.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .ready(remaining > 0);
                }
                last_retry = Instant::now();
            }

            if button.is_low() && pressed_after_debounce(&button) {
                led_test_active.store(false, Ordering::Relaxed);
                if let Err(error) = lab.stop_preview() {
                    eprintln!("stop lab preview for physical capture: {error:#}");
                }
                self.capture_cycle(&leds);
                wait_for_release(&button);
                last_retry = Instant::now();
            }
            thread::sleep(BUTTON_POLL_INTERVAL);
        }
    }

    fn capture_cycle(&self, leds: &Arc<Mutex<LedPanel>>) {
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        set_runtime_status(
            &self.runtime_status,
            "countdown",
            "Get ready — autofocus starting",
        );
        let pending = match self.camera.start_capture() {
            Ok(pending) => pending,
            Err(error) => {
                eprintln!("capture failed: {error:#}");
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("capture failed: {error:#}"),
                );
                leds.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
                return;
            }
        };
        leds.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .countdown();
        set_runtime_status(
            &self.runtime_status,
            "capturing",
            "Countdown complete — taking the photo",
        );
        let capture = match pending.finish() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("capture failed: {error:#}");
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("capture failed: {error:#}"),
                );
                leds.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
                return;
            }
        };

        set_runtime_status(
            &self.runtime_status,
            "processing",
            "Uploading the new photo",
        );
        let mut pulse = leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin_processing();
        let upload = leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .while_processing(&mut pulse, || self.uploader.upload(&capture));
        match upload {
            Ok(()) => {
                set_runtime_status(
                    &self.runtime_status,
                    "success",
                    "Photo uploaded successfully",
                );
                leds.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .success();
                set_runtime_status(&self.runtime_status, "ready", "Ready");
            }
            Err(error) => {
                eprintln!("upload failed; retained for retry: {error:#}");
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("upload failed; retained for retry: {error:#}"),
                );
                leds.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
            }
        }
    }

    fn start_admin_server(
        &self,
        leds: Arc<Mutex<LedPanel>>,
        led_test_active: Arc<AtomicBool>,
        lab: CameraLab,
    ) -> Result<()> {
        let listener = std::net::TcpListener::bind(&self.config.admin_bind)
            .with_context(|| format!("bind admin server to {}", self.config.admin_bind))?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let state = AdminState {
            config: self.config.clone(),
            camera: self.camera.clone(),
            uploader: self.uploader.clone(),
            operation_lock: Arc::clone(&self.operation_lock),
            runtime_status: self.runtime_status.clone(),
            leds,
            led_test_active,
            lab,
            started_at: self.started_at,
        };

        thread::Builder::new()
            .name("daily-mirror-admin".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build admin Tokio runtime");
                runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener)
                        .expect("adopt admin TCP listener");
                    let app = admin_router(state);
                    if let Err(error) = axum::serve(listener, app).await {
                        eprintln!("admin server stopped: {error}");
                    }
                });
            })?;
        println!("admin: http://{local}");
        Ok(())
    }
}

#[derive(Clone)]
struct Camera {
    command: String,
    args: Vec<String>,
    queue_dir: PathBuf,
    camera_lock: Arc<Mutex<()>>,
    orientation: Arc<RwLock<CameraOrientation>>,
    orientation_path: PathBuf,
}

impl Camera {
    fn new(config: &Config) -> Result<Self> {
        fs::create_dir_all(&config.queue_dir)
            .with_context(|| format!("create queue directory {}", config.queue_dir.display()))?;
        let orientation = load_orientation(&config.camera_settings_path)?;
        Ok(Self {
            command: config.camera_command.clone(),
            args: config.camera_args.clone(),
            queue_dir: config.queue_dir.clone(),
            camera_lock: Arc::new(Mutex::new(())),
            orientation: Arc::new(RwLock::new(orientation)),
            orientation_path: config.camera_settings_path.clone(),
        })
    }

    fn capture(&self) -> Result<PathBuf> {
        self.start_capture()?.finish()
    }

    fn start_capture(&self) -> Result<PendingCapture<'_>> {
        let _camera = self
            .camera_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = capture_id();
        let temporary = self.queue_dir.join(format!(".{id}.tmp"));
        let final_path = self.queue_dir.join(format!("{id}.jpg"));
        // Orientation is deliberately NOT passed to the camera: sensor-side
        // flips change the IMX519 Bayer phase and break autofocus. The JPEG
        // is transformed losslessly after capture instead.
        let orientation = self
            .orientation
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let child = Command::new(&self.command)
            .args(&self.args)
            .arg("--output")
            .arg(&temporary)
            .spawn()
            .with_context(|| format!("run camera command {}", self.command))?;
        Ok(PendingCapture {
            _camera,
            child,
            temporary,
            final_path,
            queue_dir: &self.queue_dir,
            orientation,
        })
    }

    fn finish_capture(
        queue_dir: &Path,
        temporary: &Path,
        final_path: &Path,
        status: std::process::ExitStatus,
        orientation: &CameraOrientation,
    ) -> Result<PathBuf> {
        if !status.success() {
            let _ = fs::remove_file(temporary);
            bail!("camera command exited with {status}");
        }
        validate_jpeg_file(temporary)?;
        apply_orientation(temporary, orientation);
        File::options().write(true).open(temporary)?.sync_all()?;
        fs::rename(temporary, final_path).with_context(|| {
            format!(
                "commit captured photo {} to {}",
                temporary.display(),
                final_path.display()
            )
        })?;
        sync_directory(queue_dir)?;
        Ok(final_path.to_owned())
    }

    fn capture_lab(&self, settings: &LabSettings) -> Result<(Vec<u8>, Option<serde_json::Value>)> {
        settings.validate()?;
        let _camera = self
            .camera_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let metadata_path =
            std::env::temp_dir().join(format!("daily-mirror-lab-{}.json", capture_id()));
        let output = Command::new(&self.command)
            .args([
                "--nopreview",
                "--timeout",
                "3000",
                "--width",
                "4656",
                "--height",
                "3496",
                "--encoding",
                "jpg",
                "--quality",
                "95",
            ])
            .args(settings.still_camera_args())
            .args(settings.focus_args(true))
            .args(["--metadata-format", "json", "--metadata"])
            .arg(&metadata_path)
            .args(["--output", "-"])
            .output();
        let metadata = fs::read_to_string(&metadata_path).ok().map(|raw| {
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({ "Raw": raw.trim() }))
        });
        let _ = fs::remove_file(&metadata_path);
        let output = output.with_context(|| format!("run lab capture with {}", self.command))?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            let tail = error.chars().rev().take(800).collect::<String>();
            let tail = tail.chars().rev().collect::<String>();
            bail!("lab camera exited with {}: {tail}", output.status);
        }
        validate_jpeg_bytes(&output.stdout)?;
        let mut bytes = output.stdout;
        let orientation = settings.orientation();
        if orientation.jpegtran_args().is_some() {
            let unrotated =
                std::env::temp_dir().join(format!("daily-mirror-lab-{}.jpg", capture_id()));
            if fs::write(&unrotated, &bytes).is_ok() {
                apply_orientation(&unrotated, &orientation);
                if let Ok(oriented) = fs::read(&unrotated) {
                    bytes = oriented;
                }
                let _ = fs::remove_file(&unrotated);
            }
        }
        Ok((bytes, metadata))
    }

    fn queue_existing(&self, source: &Path) -> Result<PathBuf> {
        validate_jpeg_file(source)?;
        let id = capture_id();
        let temporary = self.queue_dir.join(format!(".{id}.tmp"));
        let final_path = self.queue_dir.join(format!("{id}.jpg"));
        fs::copy(source, &temporary)
            .with_context(|| format!("copy {} into the durable upload queue", source.display()))?;
        File::options().write(true).open(&temporary)?.sync_all()?;
        fs::rename(&temporary, &final_path)?;
        sync_directory(&self.queue_dir)?;
        Ok(final_path)
    }
}

struct PendingCapture<'a> {
    _camera: std::sync::MutexGuard<'a, ()>,
    child: Child,
    temporary: PathBuf,
    final_path: PathBuf,
    queue_dir: &'a Path,
    orientation: CameraOrientation,
}

impl PendingCapture<'_> {
    fn finish(mut self) -> Result<PathBuf> {
        let status = self.child.wait().context("wait for camera command")?;
        Camera::finish_capture(
            self.queue_dir,
            &self.temporary,
            &self.final_path,
            status,
            &self.orientation,
        )
    }
}

/// Apply the saved orientation to a captured JPEG losslessly with jpegtran.
/// Best-effort: an unrotated photo beats a failed capture, so problems are
/// logged and the original file is kept.
fn apply_orientation(path: &Path, orientation: &CameraOrientation) {
    let Some(op) = orientation.jpegtran_args() else {
        return;
    };
    let transformed = path.with_extension("oriented");
    let result = Command::new("jpegtran")
        .args(["-copy", "all", "-trim"])
        .args(op)
        .arg("-outfile")
        .arg(&transformed)
        .arg(path)
        .output();
    match result {
        Ok(output) if output.status.success() && validate_jpeg_file(&transformed).is_ok() => {
            if let Err(error) = fs::rename(&transformed, path) {
                eprintln!("keeping unrotated capture: {error}");
                let _ = fs::remove_file(&transformed);
            }
        }
        Ok(output) => {
            eprintln!(
                "keeping unrotated capture: jpegtran exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            let _ = fs::remove_file(&transformed);
        }
        Err(error) => {
            eprintln!("keeping unrotated capture: jpegtran unavailable: {error}");
        }
    }
}

#[derive(Clone)]
struct Uploader {
    client: Client,
    upload_url: Option<String>,
    upload_token: Option<String>,
    queue_dir: PathBuf,
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
    headers: BTreeMap<String, String>,
    complete_url: Option<String>,
}

impl Uploader {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .build()?,
            upload_url: config.upload_url().ok(),
            upload_token: config.upload_token.clone(),
            queue_dir: config.queue_dir.clone(),
        })
    }

    fn upload(&self, path: &Path) -> Result<()> {
        let grant_url = self
            .upload_url
            .as_deref()
            .ok_or_else(|| anyhow!("DAILY_MIRROR_SERVER_URL is required for uploads"))?;
        let grant_endpoint = Url::parse(grant_url)
            .with_context(|| format!("invalid Daily Mirror server URL: {grant_url}"))?;
        let capture_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("queued photo has an invalid filename: {}", path.display()))?;
        let size = path.metadata()?.len();
        let mut grant_request =
            self.client
                .post(grant_endpoint.clone())
                .json(&UploadGrantRequest {
                    capture_id,
                    content_type: "image/jpeg",
                    content_length: size,
                });
        if let Some(token) = &self.upload_token {
            grant_request = grant_request.header(AUTHORIZATION, format!("Bearer {token}"));
        }

        let response = grant_request
            .send()
            .context("request a signed upload from the Daily Mirror server")?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().unwrap_or_default();
            bail!(
                "server rejected the upload request with HTTP {status}: {}",
                detail.trim()
            );
        }
        let grant: UploadGrant = response
            .json()
            .context("decode the Daily Mirror upload grant")?;
        let target = grant_endpoint
            .join(&grant.url)
            .context("resolve the signed upload target")?;
        let target_is_server = same_origin(&grant_endpoint, &target);
        let method = match grant.method.as_str() {
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            other => bail!("server returned unsupported upload method {other:?}"),
        };

        let file = File::open(path)?;
        let mut upload_request = self
            .client
            .request(method, target)
            .header(CONTENT_LENGTH, size)
            .body(file);
        for (name, value) in grant.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("server returned invalid upload header {name:?}"))?;
            let value = HeaderValue::from_str(&value)
                .context("server returned an invalid upload header value")?;
            upload_request = upload_request.header(name, value);
        }
        if target_is_server {
            if let Some(token) = &self.upload_token {
                upload_request = upload_request.header(AUTHORIZATION, format!("Bearer {token}"));
            }
        }

        let response = upload_request
            .send()
            .context("send photo to the signed upload target")?;
        if !response.status().is_success() {
            bail!(
                "upload target rejected photo with HTTP {}",
                response.status()
            );
        }
        if let Some(complete_url) = grant.complete_url {
            let complete_endpoint = grant_endpoint
                .join(&complete_url)
                .context("resolve the upload completion endpoint")?;
            let mut complete_request = self.client.post(complete_endpoint);
            if let Some(token) = &self.upload_token {
                complete_request =
                    complete_request.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let completion = complete_request
                .send()
                .context("confirm the completed upload")?;
            if !completion.status().is_success() {
                bail!(
                    "server rejected upload completion with HTTP {}",
                    completion.status()
                );
            }
        }
        fs::remove_file(path)
            .with_context(|| format!("remove uploaded queue file {}", path.display()))?;
        sync_directory(&self.queue_dir)?;
        Ok(())
    }

    fn retry_all(&self) -> Result<usize> {
        let mut queued = fs::read_dir(&self.queue_dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jpg"))
            .collect::<Vec<_>>();
        queued.sort();

        for path in queued {
            if let Err(error) = self.upload(&path) {
                eprintln!("retry deferred for {}: {error:#}", path.display());
            }
        }
        self.pending_count()
    }

    fn pending_count(&self) -> Result<usize> {
        Ok(fs::read_dir(&self.queue_dir)?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("jpg")
            })
            .count())
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[derive(Clone)]
struct AdminState {
    config: Config,
    camera: Camera,
    uploader: Uploader,
    operation_lock: Arc<Mutex<()>>,
    runtime_status: watch::Sender<RuntimeStatus>,
    leds: Arc<Mutex<LedPanel>>,
    led_test_active: Arc<AtomicBool>,
    lab: CameraLab,
    started_at: Instant,
}

#[derive(Clone, Debug, Serialize)]
struct RuntimeStatus {
    phase: String,
    message: String,
    green_on: bool,
    yellow_on: bool,
    red_on: bool,
    revision: u64,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            phase: "starting".to_owned(),
            message: "Starting Daily Mirror".to_owned(),
            green_on: false,
            yellow_on: false,
            red_on: false,
            revision: 0,
        }
    }
}

#[derive(Serialize)]
struct AdminStatus {
    hostname: String,
    phase: String,
    message: String,
    uptime_seconds: u64,
    pending_photos: usize,
    camera_command: String,
    camera_available: bool,
    server_url: Option<String>,
    button_pin: u8,
    green_led_pin: u8,
    yellow_led_pin: u8,
    red_led_pin: u8,
    green_on: bool,
    yellow_on: bool,
    red_on: bool,
    revision: u64,
    cpu_temperature_celsius: Option<f64>,
    load_average_one_minute: Option<f64>,
    memory_used_bytes: Option<u64>,
    memory_total_bytes: Option<u64>,
    storage_used_bytes: Option<u64>,
    storage_total_bytes: Option<u64>,
    storage_available_bytes: Option<u64>,
    lab_preview_running: bool,
    lab_preview_error: Option<String>,
    lab_has_capture: bool,
    lab_capture_metadata: Option<serde_json::Value>,
    lab_settings: LabSettings,
}

#[derive(Serialize)]
struct ActionResponse {
    ok: bool,
    message: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    software_version: &'static str,
    phase: String,
    camera_available: bool,
    upload_configured: bool,
    pending_photos: usize,
    revision: u64,
    cpu_temperature_celsius: Option<f64>,
    storage_available_bytes: Option<u64>,
}

impl AdminState {
    fn snapshot(&self) -> AdminStatus {
        let runtime = self.runtime_status.borrow().clone();
        let (memory_used_bytes, memory_total_bytes) = memory_usage()
            .map(|(used, total)| (Some(used), Some(total)))
            .unwrap_or_default();
        let (storage_used_bytes, storage_total_bytes, storage_available_bytes) =
            filesystem_usage(&self.config.queue_dir)
                .map(|(used, total, available)| (Some(used), Some(total), Some(available)))
                .unwrap_or_default();
        AdminStatus {
            hostname: fs::read_to_string("/etc/hostname")
                .unwrap_or_else(|_| "raspberry-pi".to_owned())
                .trim()
                .to_owned(),
            phase: runtime.phase,
            message: runtime.message,
            uptime_seconds: self.started_at.elapsed().as_secs(),
            pending_photos: self.uploader.pending_count().unwrap_or_default(),
            camera_available: Path::new(&self.camera.command).exists(),
            camera_command: self.camera.command.clone(),
            server_url: self.config.server_url.clone(),
            button_pin: self.config.button_pin,
            green_led_pin: self.config.green_led_pin,
            yellow_led_pin: self.config.yellow_led_pin,
            red_led_pin: self.config.red_led_pin,
            green_on: runtime.green_on,
            yellow_on: runtime.yellow_on,
            red_on: runtime.red_on,
            revision: runtime.revision,
            cpu_temperature_celsius: cpu_temperature_celsius(),
            load_average_one_minute: load_average_one_minute(),
            memory_used_bytes,
            memory_total_bytes,
            storage_used_bytes,
            storage_total_bytes,
            storage_available_bytes,
            lab_preview_running: self.lab.preview_running(),
            lab_preview_error: self.lab.preview_error(),
            lab_has_capture: self.lab.has_capture(),
            lab_capture_metadata: self.lab.capture_metadata(),
            lab_settings: self.lab.settings(),
        }
    }

    fn capture_and_upload(&self) -> Result<String> {
        self.lab.stop_preview()?;
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.led_test_active.store(false, Ordering::Relaxed);
        set_runtime_status(
            &self.runtime_status,
            "countdown",
            "Get ready — autofocus starting",
        );
        let pending = self.camera.start_capture()?;
        self.leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .countdown();
        set_runtime_status(
            &self.runtime_status,
            "capturing",
            "Countdown complete — taking the photo",
        );
        let result = (|| {
            let capture = pending.finish()?;
            let capture_id = capture
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("new photo")
                .to_owned();
            set_runtime_status(
                &self.runtime_status,
                "processing",
                "Uploading the new photo",
            );
            let mut pulse = self
                .leds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .begin_processing();
            self.leds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .while_processing(&mut pulse, || self.uploader.upload(&capture))?;
            Ok(format!("Captured and uploaded {capture_id}"))
        })();

        match &result {
            Ok(message) => {
                set_runtime_status(&self.runtime_status, "success", message);
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .success();
                set_runtime_status(&self.runtime_status, "ready", "Ready");
            }
            Err(error) => {
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .finish_processing();
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("capture/upload failed: {error:#}"),
                );
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
            }
        }
        result
    }

    fn start_lab_preview(&self) -> Result<String> {
        let message = self.lab.start_preview()?;
        set_runtime_status(&self.runtime_status, "live preview", &message);
        Ok(message)
    }

    fn stop_lab_preview(&self) -> Result<String> {
        let message = self.lab.stop_preview()?;
        set_runtime_status(&self.runtime_status, "camera lab", &message);
        Ok(message)
    }

    fn apply_lab_settings(&self, settings: LabSettings) -> Result<String> {
        let message = self.lab.apply_settings(settings)?;
        set_runtime_status(&self.runtime_status, "camera lab", &message);
        Ok(message)
    }

    fn capture_lab_photo(&self) -> Result<String> {
        self.lab.stop_preview()?;
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.led_test_active.store(false, Ordering::Relaxed);
        set_runtime_status(
            &self.runtime_status,
            "lab capture",
            "Hold still — capturing a full-resolution test image",
        );
        let mut pulse = self
            .leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin_processing();
        let result = self
            .leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .while_processing(&mut pulse, || {
                self.camera
                    .capture_lab(&self.lab.settings())
                    .map(|(jpeg, metadata)| {
                        let size = jpeg.len();
                        self.lab.store_capture(jpeg, metadata);
                        format!(
                            "Test photo captured in memory ({:.1} MB); it was not uploaded",
                            size as f64 / 1024.0 / 1024.0
                        )
                    })
            });
        match &result {
            Ok(message) => {
                set_runtime_status(&self.runtime_status, "lab captured", message);
                let pending = self.uploader.pending_count().unwrap_or_default();
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .ready(pending > 0);
            }
            Err(error) => {
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("lab capture failed: {error:#}"),
                );
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
            }
        }
        result
    }

    fn retry_uploads(&self) -> Result<String> {
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.led_test_active.store(false, Ordering::Relaxed);
        let mut pulse = self
            .leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin_processing();
        set_runtime_status(&self.runtime_status, "uploading", "Retrying queued uploads");
        let result = self
            .leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .while_processing(&mut pulse, || self.uploader.retry_all())
            .map(|remaining| format!("Retry finished; {remaining} photo(s) remain queued"));
        match &result {
            Ok(message) => {
                let remaining = self.uploader.pending_count().unwrap_or(1);
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .ready(remaining > 0);
                set_runtime_status(&self.runtime_status, "ready", message);
            }
            Err(error) => {
                set_runtime_status(
                    &self.runtime_status,
                    "error",
                    format!("retry failed: {error:#}"),
                );
                self.leds
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .error();
            }
        }
        result
    }

    fn test_led(&self, mode: &str) -> Result<String> {
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if mode == "restore" {
            self.led_test_active.store(false, Ordering::Relaxed);
            let pending = self.uploader.pending_count().unwrap_or_default();
            self.leds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .ready(pending > 0);
            let message = if pending == 0 {
                "Ready".to_owned()
            } else {
                format!("Ready; {pending} photo(s) waiting to upload")
            };
            set_runtime_status(&self.runtime_status, "ready", &message);
            return Ok(message);
        }

        let (green_on, yellow_on, red_on, label) = match mode {
            "green" => (true, false, false, "green"),
            "yellow" => (false, true, false, "yellow"),
            "red" => (false, false, true, "red"),
            "off" => (false, false, false, "all LEDs off"),
            _ => bail!("unknown LED test mode: {mode}"),
        };
        self.led_test_active.store(true, Ordering::Relaxed);
        set_runtime_status(
            &self.runtime_status,
            "LED test",
            format!("Manual LED test: {label}"),
        );
        self.leds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set(green_on, yellow_on, red_on);
        Ok(format!("Manual LED test set to {label}"))
    }
}

fn admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/", get(admin_page))
        .route("/healthz", get(admin_health))
        .route("/api/status", get(admin_status))
        .route("/api/events", get(admin_events))
        .route("/api/actions/capture", post(admin_capture))
        .route("/api/actions/retry", post(admin_retry))
        .route("/api/actions/led/{mode}", post(admin_led))
        .route("/api/lab/preview/start", post(admin_lab_preview_start))
        .route("/api/lab/preview/stop", post(admin_lab_preview_stop))
        .route("/api/lab/settings", post(admin_lab_settings))
        .route("/api/lab/capture", post(admin_lab_capture))
        .route("/api/lab/frame.jpg", get(admin_lab_frame))
        .route("/api/lab/capture.jpg", get(admin_lab_capture_image))
        .with_state(state)
}

async fn admin_page() -> Html<&'static str> {
    Html(ADMIN_PAGE)
}

async fn admin_status(State(state): State<AdminState>) -> Json<AdminStatus> {
    Json(state.snapshot())
}

async fn admin_health(State(state): State<AdminState>) -> (StatusCode, Json<HealthResponse>) {
    let snapshot = state.snapshot();
    let upload_configured = snapshot.server_url.is_some();
    let healthy = snapshot.camera_available
        && upload_configured
        && !matches!(snapshot.phase.as_str(), "starting" | "error");
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(HealthResponse {
            status: if healthy { "ok" } else { "degraded" },
            software_version: env!("CARGO_PKG_VERSION"),
            phase: snapshot.phase,
            camera_available: snapshot.camera_available,
            upload_configured,
            pending_photos: snapshot.pending_photos,
            revision: snapshot.revision,
            cpu_temperature_celsius: snapshot.cpu_temperature_celsius,
            storage_available_bytes: snapshot.storage_available_bytes,
        }),
    )
}

async fn admin_events(State(state): State<AdminState>) -> impl IntoResponse {
    let mut receiver = state.runtime_status.subscribe();
    let stream = async_stream::stream! {
        yield Ok::<Event, Infallible>(Event::default()
            .json_data(state.snapshot())
            .expect("serialize initial admin status"));

        while receiver.changed().await.is_ok() {
            let _revision = receiver.borrow_and_update().revision;
            yield Ok(Event::default()
                .json_data(state.snapshot())
                .expect("serialize admin status"));
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("daily-mirror"),
    )
}

async fn admin_capture(State(state): State<AdminState>) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.capture_and_upload()).await;
    action_response(result)
}

async fn admin_retry(State(state): State<AdminState>) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.retry_uploads()).await;
    action_response(result)
}

async fn admin_led(
    AxumPath(mode): AxumPath<String>,
    State(state): State<AdminState>,
) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.test_led(&mode)).await;
    action_response(result)
}

async fn admin_lab_preview_start(
    State(state): State<AdminState>,
) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.start_lab_preview()).await;
    action_response(result)
}

async fn admin_lab_preview_stop(
    State(state): State<AdminState>,
) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.stop_lab_preview()).await;
    action_response(result)
}

async fn admin_lab_settings(
    State(state): State<AdminState>,
    Json(settings): Json<LabSettings>,
) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.apply_lab_settings(settings)).await;
    action_response(result)
}

async fn admin_lab_capture(State(state): State<AdminState>) -> (StatusCode, Json<ActionResponse>) {
    let result = tokio::task::spawn_blocking(move || state.capture_lab_photo()).await;
    action_response(result)
}

async fn admin_lab_frame(State(state): State<AdminState>) -> Response {
    jpeg_response(state.lab.preview_frame())
}

async fn admin_lab_capture_image(State(state): State<AdminState>) -> Response {
    jpeg_response(state.lab.capture())
}

fn jpeg_response(jpeg: Option<Vec<u8>>) -> Response {
    match jpeg {
        Some(jpeg) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "image/jpeg")
            .header("cache-control", "no-store, max-age=0")
            .body(Body::from(jpeg))
            .expect("build JPEG response"),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

fn action_response(
    result: std::result::Result<Result<String>, tokio::task::JoinError>,
) -> (StatusCode, Json<ActionResponse>) {
    match result {
        Ok(Ok(message)) => (StatusCode::OK, Json(ActionResponse { ok: true, message })),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ActionResponse {
                ok: false,
                message: format!("{error:#}"),
            }),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ActionResponse {
                ok: false,
                message: format!("admin task failed: {error}"),
            }),
        ),
    }
}

fn set_runtime_status(
    status: &watch::Sender<RuntimeStatus>,
    phase: impl Into<String>,
    message: impl Into<String>,
) {
    let phase = phase.into();
    let message = message.into();
    status.send_modify(|runtime| {
        runtime.phase = phase;
        runtime.message = message;
        runtime.revision = runtime.revision.wrapping_add(1);
    });
}

fn set_led_status(
    status: &watch::Sender<RuntimeStatus>,
    green_on: bool,
    yellow_on: bool,
    red_on: bool,
) {
    status.send_modify(|runtime| {
        runtime.green_on = green_on;
        runtime.yellow_on = yellow_on;
        runtime.red_on = red_on;
        runtime.revision = runtime.revision.wrapping_add(1);
    });
}

const ADMIN_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Daily Mirror · Pi admin</title>
  <style>
    :root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }
    * { box-sizing: border-box; }
    [hidden] { display: none !important; }
    body { margin: 0; min-height: 100vh; background: #0b1110; color: #eaf4ef; }
    main { width: min(1080px, calc(100% - 32px)); margin: 0 auto; padding: 48px 0 72px; }
    header { display: flex; justify-content: space-between; gap: 24px; align-items: end; margin-bottom: 28px; }
    .eyebrow { color: #7fd8a8; font-size: 12px; letter-spacing: .16em; text-transform: uppercase; }
    h1 { margin: 7px 0 0; font-size: clamp(32px, 7vw, 64px); line-height: .95; font-weight: 620; letter-spacing: -.04em; }
    .badge { border: 1px solid #2b4639; border-radius: 999px; padding: 9px 13px; color: #a9bdb3; white-space: nowrap; }
    .badge::before { content: ''; display: inline-block; width: 8px; height: 8px; margin-right: 8px; border-radius: 50%; background: #7fd8a8; box-shadow: 0 0 16px #7fd8a8; }
    .hero, .card { border: 1px solid #22342d; background: #101a17; border-radius: 20px; }
    .hero { padding: 28px; margin-bottom: 16px; }
    .phase { color: #7fd8a8; font-size: 13px; text-transform: uppercase; letter-spacing: .12em; }
    .message { margin: 9px 0 0; font-size: clamp(22px, 4vw, 36px); line-height: 1.12; }
    .signals { display: flex; gap: 22px; margin-top: 26px; }
    .signal { display: grid; justify-items: center; gap: 8px; color: #71857b; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; }
    .lamp { display: block; width: 26px; height: 26px; border: 1px solid #3a4741; border-radius: 50%; background: #17201c; transition: background 100ms ease, border-color 100ms ease, box-shadow 100ms ease, transform 100ms ease; }
    .lamp.on { transform: scale(1.05); }
    .lamp.green.on { background: #59e99b; border-color: #a8ffd0; box-shadow: 0 0 22px #39d880; }
    .lamp.yellow.on { background: #ffd35a; border-color: #ffe7a3; box-shadow: 0 0 22px #dba817; }
    .lamp.red.on { background: #ff6262; border-color: #ffabab; box-shadow: 0 0 22px #e32e3b; }
    .grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 16px; }
    .card { padding: 22px; }
    .card.wide { grid-column: 1 / -1; }
    .card h2 { margin: 0 0 18px; color: #9cb1a7; font-size: 13px; font-weight: 500; text-transform: uppercase; letter-spacing: .1em; }
    dl { display: grid; grid-template-columns: 1fr auto; gap: 11px 16px; margin: 0; }
    dt { color: #81958b; } dd { margin: 0; text-align: right; font-variant-numeric: tabular-nums; }
    .actions { display: flex; flex-wrap: wrap; gap: 10px; }
    button, a.button { appearance: none; border: 0; border-radius: 11px; padding: 12px 16px; background: #78dfa8; color: #08110d; font: inherit; font-weight: 650; cursor: pointer; text-decoration: none; }
    button.secondary, a.secondary { background: #22362d; color: #dcebe3; }
    button.test-yellow { background: #ffd35a; color: #251b00; }
    button.test-red { background: #ef5c64; color: white; }
    button:disabled { cursor: wait; opacity: .5; }
    .result { min-height: 22px; margin: 16px 0 0; color: #9cb1a7; line-height: 1.4; }
    .diagnostics { margin-top: 16px; }
    .hint { margin: 14px 0 0; color: #81958b; line-height: 1.45; }
    .lab { margin-top: 16px; }
    .lab-layout { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(300px, .65fr); gap: 22px; align-items: start; }
    .lab-screen { position: relative; aspect-ratio: 4 / 3; display: grid; place-items: center; overflow: hidden; border: 1px solid #263a31; border-radius: 14px; background: #070b09; }
    .lab-screen img { width: 100%; height: 100%; display: block; object-fit: contain; }
    .focus-window { position: absolute; z-index: 2; border: 2px solid #78dfa8; border-radius: 8px; box-shadow: 0 0 0 1px #07100c, 0 0 18px rgba(120, 223, 168, .35); pointer-events: none; }
    .focus-window::after { content: "AF"; position: absolute; top: -2px; left: -2px; padding: 3px 6px; border-radius: 6px 0 6px 0; background: #78dfa8; color: #08110d; font-size: 10px; font-weight: 800; }
    .lab-placeholder { max-width: 32ch; padding: 24px; color: #71857b; line-height: 1.5; text-align: center; }
    .lab-toolbar { margin-top: 12px; }
    .lab-status { min-height: 22px; margin: 12px 0 0; color: #9cb1a7; line-height: 1.4; }
    .settings { display: grid; gap: 14px; }
    .setting-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
    .setting { display: grid; gap: 7px; color: #81958b; font-size: 12px; }
    .setting-name { display: flex; justify-content: space-between; gap: 8px; }
    input, select { width: 100%; min-width: 0; border: 1px solid #30473c; border-radius: 9px; padding: 9px 10px; background: #0b1410; color: #eaf4ef; font: inherit; }
    input[type="range"] { padding: 0; accent-color: #78dfa8; }
    input[type="checkbox"] { width: auto; margin-right: 7px; accent-color: #78dfa8; }
    .setting output { color: #c5d8ce; font-variant-numeric: tabular-nums; }
    .focus-heading, .focus-presets { grid-column: 1 / -1; }
    .focus-heading { display: flex; justify-content: space-between; gap: 12px; padding-top: 5px; color: #9cb1a7; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; }
    .focus-heading span { color: #61766b; letter-spacing: 0; text-transform: none; }
    .focus-presets { display: flex; flex-wrap: wrap; gap: 7px; }
    .focus-presets button { padding: 8px 10px; border: 1px solid #30473c; background: #17261f; color: #cfe2d8; font-size: 11px; }
    .focus-results { margin-top: 14px; padding: 14px; border: 1px solid #263a31; border-radius: 12px; background: #0b1410; }
    .focus-results h3 { margin: 0 0 11px; color: #9cb1a7; font-size: 11px; font-weight: 600; letter-spacing: .1em; text-transform: uppercase; }
    .focus-metadata { grid-template-columns: repeat(2, 1fr); gap: 8px 14px; font-size: 12px; }
    .focus-metadata dd { color: #dcebe3; }
    .metadata-raw { margin-top: 12px; color: #81958b; font-size: 11px; }
    .metadata-raw summary { cursor: pointer; }
    .metadata-raw pre { max-height: 240px; overflow: auto; white-space: pre-wrap; color: #a9bdb3; }
    .lab-note { margin: 0; color: #71857b; font-size: 12px; line-height: 1.45; }
    footer { margin-top: 18px; color: #687c72; font-size: 13px; }
    @media (max-width: 760px) { header { display: block; } .badge { display: inline-block; margin-top: 18px; } .grid, .lab-layout { grid-template-columns: 1fr; } .card.wide { grid-column: auto; } }
  </style>
</head>
<body>
  <main>
    <header><div><div class="eyebrow">Raspberry Pi control plane</div><h1>Daily Mirror</h1></div><div class="badge" id="host">connecting</div></header>
    <section class="hero">
      <div class="phase" id="phase">starting</div>
      <p class="message" id="message">Connecting to the device…</p>
      <div class="signals" aria-label="Physical LED state">
        <div class="signal"><span class="lamp green" id="green-led"></span><span>Ready / processing</span></div>
        <div class="signal"><span class="lamp yellow" id="yellow-led"></span><span>Countdown</span></div>
        <div class="signal"><span class="lamp red" id="red-led"></span><span>Error</span></div>
      </div>
    </section>
    <div class="grid">
      <section class="card">
        <h2>Device</h2>
        <dl>
          <dt>Uptime</dt><dd id="uptime">—</dd>
          <dt>Camera</dt><dd id="camera">—</dd>
          <dt>Queued photos</dt><dd id="pending">—</dd>
          <dt>GPIO</dt><dd id="gpio">—</dd>
        </dl>
      </section>
      <section class="card">
        <h2>System</h2>
        <dl>
          <dt>Temperature</dt><dd id="temperature">—</dd>
          <dt>Load · 1 min</dt><dd id="load">—</dd>
          <dt>Memory</dt><dd id="memory">—</dd>
          <dt>Storage</dt><dd id="storage">—</dd>
          <dt>Storage free</dt><dd id="storage-free">—</dd>
        </dl>
      </section>
      <section class="card wide">
        <h2>Controls</h2>
        <div class="actions">
          <button data-action="/api/actions/capture">Capture now</button>
          <button class="secondary" data-action="/api/actions/retry">Retry uploads</button>
          <a class="button secondary" id="gallery" target="_blank" rel="noreferrer">Open gallery</a>
        </div>
        <p class="result" id="result">Controls operate the same durable queue as the physical button.</p>
      </section>
    </div>
    <section class="card lab">
      <h2>Camera lab · local only</h2>
      <div class="lab-layout">
        <div>
          <div class="lab-screen">
            <img id="lab-viewer" alt="Camera lab preview" hidden>
            <div class="focus-window" id="focus-window" aria-hidden="true"></div>
            <div class="lab-placeholder" id="lab-placeholder">Start live preview to tune the camera, or take a full-resolution test snap. Lab images stay in memory and are never queued or uploaded.</div>
          </div>
          <div class="actions lab-toolbar">
            <button data-action="/api/lab/preview/start" data-result="lab-result">Start live preview</button>
            <button class="secondary" data-action="/api/lab/preview/stop" data-result="lab-result">Stop preview</button>
            <button class="secondary" data-action="/api/lab/capture" data-result="lab-result">Apply + test snap</button>
            <a class="button secondary" id="lab-full-resolution" href="/api/lab/capture.jpg" target="_blank" rel="noreferrer" hidden>Open full resolution</a>
          </div>
          <p class="lab-status" id="lab-result">Preview is stopped.</p>
          <section class="focus-results" id="focus-results" hidden>
            <h3>Last test snap · camera metadata</h3>
            <dl class="focus-metadata">
              <dt>AF state</dt><dd id="focus-state">—</dd>
              <dt>Lens position</dt><dd id="focus-lens-position">—</dd>
              <dt>Focus score</dt><dd id="focus-score">—</dd>
              <dt>Exposure</dt><dd id="focus-exposure">—</dd>
              <dt>Analogue gain</dt><dd id="focus-gain">—</dd>
            </dl>
            <details class="metadata-raw"><summary>All capture metadata</summary><pre id="focus-metadata-raw"></pre></details>
          </section>
        </div>
        <form class="settings" id="lab-settings">
          <div class="setting-grid">
            <label class="setting"><span>Camera rotation<select name="rotation_degrees"><option value="0">Normal · 0°</option><option value="180">Upside down · 180°</option></select></span></label>
            <label class="setting"><span><input name="hflip" type="checkbox">Mirror horizontally</span></label>
            <label class="setting"><span><input name="vflip" type="checkbox">Mirror vertically</span></label>
            <label class="setting"><span class="setting-name"><span>Exposure compensation</span><output id="ev-value">0</output></span><input name="ev" type="range" min="-4" max="4" step="0.25"></label>
            <label class="setting"><span class="setting-name"><span>Brightness</span><output id="brightness-value">0</output></span><input name="brightness" type="range" min="-0.5" max="0.5" step="0.05"></label>
            <label class="setting"><span class="setting-name"><span>Contrast</span><output id="contrast-value">1</output></span><input name="contrast" type="range" min="0" max="3" step="0.1"></label>
            <label class="setting"><span class="setting-name"><span>Saturation</span><output id="saturation-value">1</output></span><input name="saturation" type="range" min="0" max="3" step="0.1"></label>
            <label class="setting"><span class="setting-name"><span>Sharpness</span><output id="sharpness-value">1</output></span><input name="sharpness" type="range" min="0" max="4" step="0.1"></label>
            <label class="setting"><span>Denoise<select name="denoise"><option value="auto">Auto</option><option value="cdn_hq">High quality</option><option value="cdn_fast">Fast</option><option value="cdn_off">Spatial only</option><option value="off">Off</option></select></span></label>
            <label class="setting"><span>White balance<select name="awb"><option value="auto">Auto</option><option value="daylight">Daylight</option><option value="cloudy">Cloudy</option><option value="indoor">Indoor</option><option value="tungsten">Tungsten</option><option value="fluorescent">Fluorescent</option><option value="incandescent">Incandescent</option></select></span></label>
            <label class="setting"><span>Metering<select name="metering"><option value="centre">Centre weighted</option><option value="average">Whole frame</option><option value="spot">Spot</option></select></span></label>
            <label class="setting"><span>Exposure profile<select name="exposure"><option value="normal">Normal</option><option value="sport">Short exposure</option></select></span></label>
            <div class="focus-heading">Focus calibration <span>The green box is the active AF region</span></div>
            <label class="setting"><span>Focus strategy<select name="autofocus_mode"><option value="continuous">Continuous · tracks during preview</option><option value="auto">Single AF · repeats on test snap</option><option value="manual">Fixed lens position</option></select></span></label>
            <label class="setting" data-focus-control="manual"><span>Lens position · dioptres<input name="lens_position" type="number" min="0" max="32" step="0.01"></span></label>
            <label class="setting" data-focus-control="manual"><span>Approximate distance<output id="focus-distance">—</output></span></label>
            <label class="setting" data-focus-control="auto"><span>Focus range<select name="autofocus_range"><option value="normal">Normal</option><option value="full">Full</option><option value="macro">Macro</option></select></span></label>
            <label class="setting" data-focus-control="auto"><span>Focus speed<select name="autofocus_speed"><option value="normal">Normal</option><option value="fast">Fast</option></select></span></label>
            <label class="setting" data-focus-control="auto"><span>AF window X · 0–1<input name="autofocus_window_x" type="number" min="0" max="1" step="0.01"></span></label>
            <label class="setting" data-focus-control="auto"><span>AF window Y · 0–1<input name="autofocus_window_y" type="number" min="0" max="1" step="0.01"></span></label>
            <label class="setting" data-focus-control="auto"><span>AF window width<input name="autofocus_window_width" type="number" min="0.01" max="1" step="0.01"></span></label>
            <label class="setting" data-focus-control="auto"><span>AF window height<input name="autofocus_window_height" type="number" min="0.01" max="1" step="0.01"></span></label>
            <div class="focus-presets" aria-label="Focus presets">
              <button type="button" data-focus-preset="portrait">Portrait AF</button>
              <button type="button" data-focus-feet="3">Fixed · 3 ft</button>
              <button type="button" data-focus-feet="4">Fixed · 4 ft</button>
              <button type="button" data-focus-feet="5">Fixed · 5 ft</button>
            </div>
            <label class="setting"><span>Shutter · µs (0 = auto)<input name="shutter_us" type="number" min="0" max="1000000" step="1000"></span></label>
            <label class="setting"><span>Gain (0 = auto)<input name="gain" type="number" min="0" max="16" step="0.1"></span></label>
          </div>
          <div class="actions">
            <button type="submit">Apply settings</button>
            <button class="secondary" type="button" id="lab-reset">Reset controls</button>
          </div>
          <p class="lab-note">Start preview or take a test snap to apply every visible control automatically. Test snaps stay in memory and are never uploaded. The AF window uses normalized x, y, width, and height values. Fixed-distance presets convert feet to approximate dioptres; use the reported lens position and full-resolution image to calibrate the actual module. Camera orientation is saved on the Pi and applies to normal captures after restart; other lab values remain temporary.</p>
        </form>
      </div>
    </section>
    <section class="card diagnostics">
      <h2>LED test</h2>
      <div class="actions">
        <button data-action="/api/actions/led/green">Green</button>
        <button class="test-yellow" data-action="/api/actions/led/yellow">Yellow</button>
        <button class="test-red" data-action="/api/actions/led/red">Red</button>
        <button class="secondary" data-action="/api/actions/led/off">All off</button>
        <button class="secondary" data-action="/api/actions/led/restore">Restore status</button>
      </div>
      <p class="hint">A test selection stays active so you can inspect the breadboard. The physical button or Restore status returns the LEDs to normal operation.</p>
    </section>
    <footer>LAN-only prototype · live status over a persistent event stream</footer>
  </main>
  <script>
    const byId = id => document.getElementById(id);
    const duration = seconds => seconds < 60 ? `${seconds}s` : seconds < 3600 ? `${Math.floor(seconds / 60)}m ${seconds % 60}s` : `${Math.floor(seconds / 3600)}h ${Math.floor(seconds % 3600 / 60)}m`;
    const bytes = value => {
      if (value == null) return '—';
      const units = ['B', 'KB', 'MB', 'GB', 'TB'];
      let amount = value;
      let unit = 0;
      while (amount >= 1024 && unit < units.length - 1) {
        amount /= 1024;
        unit += 1;
      }
      return `${amount.toFixed(unit < 2 ? 0 : 1)} ${units[unit]}`;
    };
    const labDefaults = {
      rotation_degrees: 0, hflip: false, vflip: false,
      ev: 0, brightness: 0, contrast: 1, saturation: 1, sharpness: 1,
      denoise: 'auto', awb: 'auto', metering: 'centre', exposure: 'normal',
      shutter_us: 0, gain: 0, autofocus_mode: 'continuous',
      autofocus_range: 'normal', autofocus_speed: 'fast', lens_position: 0.82,
      autofocus_window_x: 0.2, autofocus_window_y: 0.15, autofocus_window_width: 0.6, autofocus_window_height: 0.7
    };
    const labNumericFields = new Set(['rotation_degrees', 'ev', 'brightness', 'contrast', 'saturation', 'sharpness', 'shutter_us', 'gain', 'lens_position', 'autofocus_window_x', 'autofocus_window_y', 'autofocus_window_width', 'autofocus_window_height']);
    const labBooleanFields = new Set(['hflip', 'vflip']);
    let uptimeBase = 0;
    let uptimeReceivedAt = Date.now();
    let lastStatus = null;
    let labSettingsInitialized = false;
    let labFrameLoading = false;
    let labObjectUrl = null;
    let labCaptureLoaded = false;
    function render(status) {
      lastStatus = status;
      uptimeBase = status.uptime_seconds;
      uptimeReceivedAt = Date.now();
      byId('host').textContent = status.hostname;
      byId('phase').textContent = status.phase;
      byId('message').textContent = status.message;
      byId('uptime').textContent = duration(status.uptime_seconds);
      byId('camera').textContent = status.camera_available ? 'IMX519 online' : 'unavailable';
      byId('pending').textContent = status.pending_photos;
      byId('gpio').textContent = `button ${status.button_pin} · LEDs ${status.green_led_pin}/${status.yellow_led_pin}/${status.red_led_pin}`;
      byId('temperature').textContent = status.cpu_temperature_celsius == null ? '—' : `${status.cpu_temperature_celsius.toFixed(1)} °C`;
      byId('load').textContent = status.load_average_one_minute == null ? '—' : status.load_average_one_minute.toFixed(2);
      byId('memory').textContent = `${bytes(status.memory_used_bytes)} / ${bytes(status.memory_total_bytes)}`;
      byId('storage').textContent = `${bytes(status.storage_used_bytes)} / ${bytes(status.storage_total_bytes)}`;
      byId('storage-free').textContent = bytes(status.storage_available_bytes);
      byId('green-led').classList.toggle('on', status.green_on);
      byId('yellow-led').classList.toggle('on', status.yellow_on);
      byId('red-led').classList.toggle('on', status.red_on);
      const gallery = byId('gallery');
      gallery.href = status.server_url || '#';
      gallery.hidden = !status.server_url;
      const fullResolution = byId('lab-full-resolution');
      fullResolution.hidden = !status.lab_has_capture;
      fullResolution.href = '/api/lab/capture.jpg?revision=' + status.revision;
      renderFocusMetadata(status.lab_capture_metadata);
      if (!labSettingsInitialized && status.lab_settings) {
        populateLabSettings(status.lab_settings);
        labSettingsInitialized = true;
      }
      if (status.lab_preview_error) {
        byId('lab-result').textContent = `Preview error: ${status.lab_preview_error}`;
      } else if (status.lab_preview_running) {
        labCaptureLoaded = false;
        byId('lab-result').textContent = 'Live preview active · full field of view · 960×720 at up to 8 fps';
        byId('lab-placeholder').textContent = 'Starting camera and waiting for the first frame…';
      } else if (status.lab_has_capture) {
        byId('lab-result').textContent = 'Full-resolution test photo held in memory · not uploaded';
        if (!labCaptureLoaded) {
          labCaptureLoaded = true;
          loadLabImage(`/api/lab/capture.jpg?revision=${status.revision}`, 'Full-resolution local test capture');
        }
      } else {
        byId('lab-result').textContent = 'Preview is stopped.';
      }
    }

    function metadataValue(metadata, ...keys) {
      if (!metadata || typeof metadata !== 'object') return null;
      for (const key of keys) {
        if (metadata[key] != null) return metadata[key];
        const match = Object.entries(metadata).find(([name]) => name.toLowerCase() === key.toLowerCase());
        if (match) return match[1];
      }
      return null;
    }

    function renderFocusMetadata(metadata) {
      const panel = byId('focus-results');
      panel.hidden = !metadata;
      if (!metadata) return;
      const afState = metadataValue(metadata, 'AfState');
      const afNames = { 0: 'idle', 1: 'scanning', 2: 'focused', 3: 'failed' };
      byId('focus-state').textContent = afState == null ? 'not reported' : (afNames[afState] || String(afState));
      const lens = metadataValue(metadata, 'LensPosition');
      byId('focus-lens-position').textContent = lens == null ? 'not reported' : Number(lens).toFixed(2) + ' D';
      const score = metadataValue(metadata, 'FocusFoM');
      byId('focus-score').textContent = score == null ? 'not reported' : String(score);
      const exposure = metadataValue(metadata, 'ExposureTime');
      byId('focus-exposure').textContent = exposure == null ? 'not reported' : Number(exposure).toFixed(0) + ' µs';
      const gain = metadataValue(metadata, 'AnalogueGain');
      byId('focus-gain').textContent = gain == null ? 'not reported' : Number(gain).toFixed(2);
      byId('focus-metadata-raw').textContent = JSON.stringify(metadata, null, 2);
    }

    function populateLabSettings(settings) {
      const form = byId('lab-settings');
      for (const [name, value] of Object.entries(settings)) {
        const field = form.elements.namedItem(name);
        if (!field) continue;
        if (labBooleanFields.has(name)) field.checked = Boolean(value);
        else field.value = String(value);
      }
      updateLabReadouts();
    }

    function updateLabReadouts() {
      const form = byId('lab-settings');
      for (const name of ['ev', 'brightness', 'contrast', 'saturation', 'sharpness']) {
        byId(name + '-value').textContent = form.elements.namedItem(name).value;
      }
      const manual = form.elements.namedItem('autofocus_mode').value === 'manual';
      document.querySelectorAll('[data-focus-control="manual"]').forEach(node => node.hidden = !manual);
      document.querySelectorAll('[data-focus-control="auto"]').forEach(node => node.hidden = manual);
      const lensPosition = Number(form.elements.namedItem('lens_position').value);
      byId('focus-distance').textContent = lensPosition > 0 ? (3.28084 / lensPosition).toFixed(2) + ' ft' : 'infinity';
      const focusWindow = byId('focus-window');
      focusWindow.hidden = manual;
      focusWindow.style.left = Number(form.elements.namedItem('autofocus_window_x').value) * 100 + '%';
      focusWindow.style.top = Number(form.elements.namedItem('autofocus_window_y').value) * 100 + '%';
      focusWindow.style.width = Number(form.elements.namedItem('autofocus_window_width').value) * 100 + '%';
      focusWindow.style.height = Number(form.elements.namedItem('autofocus_window_height').value) * 100 + '%';
    }

    function collectLabSettings() {
      const form = byId('lab-settings');
      const settings = {};
      for (const [name, defaultValue] of Object.entries(labDefaults)) {
        const field = form.elements.namedItem(name);
        const value = labBooleanFields.has(name) ? field.checked : field.value;
        settings[name] = labNumericFields.has(name) ? Number(value) : value;
        if (name === 'shutter_us') settings[name] = Math.round(settings[name]);
      }
      return settings;
    }

    async function loadLabImage(url, alt) {
      if (labFrameLoading) return;
      labFrameLoading = true;
      try {
        const response = await fetch(url, { cache: 'no-store' });
        if (response.status === 204) return;
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const nextUrl = URL.createObjectURL(await response.blob());
        const viewer = byId('lab-viewer');
        viewer.src = nextUrl;
        viewer.alt = alt;
        viewer.hidden = false;
        byId('lab-placeholder').hidden = true;
        if (labObjectUrl) URL.revokeObjectURL(labObjectUrl);
        labObjectUrl = nextUrl;
      } catch (error) {
        byId('lab-result').textContent = `Preview image failed: ${error.message}`;
      } finally {
        labFrameLoading = false;
      }
    }

    async function refreshLabFrame() {
      if (!lastStatus?.lab_preview_running) return;
      await loadLabImage(`/api/lab/frame.jpg?t=${Date.now()}`, 'Live IMX519 camera preview');
    }
    async function refresh() {
      try {
        const response = await fetch('/api/status', { cache: 'no-store' });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        render(await response.json());
      } catch (error) {
        byId('phase').textContent = 'offline';
        byId('message').textContent = 'The Pi admin service is not responding';
      }
    }
    async function saveLabSettings() {
      const response = await fetch('/api/lab/settings', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(collectLabSettings())
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.message || 'HTTP ' + response.status);
      labCaptureLoaded = false;
      return result.message;
    }

    async function action(button) {
      const buttons = [...document.querySelectorAll('button[data-action]')];
      const resultElement = byId(button.dataset.result || 'result');
      const actionPath = button.dataset.action;
      buttons.forEach(item => item.disabled = true);
      resultElement.textContent = 'Working…';
      try {
        if (actionPath === '/api/lab/preview/start' || actionPath === '/api/lab/capture') {
          resultElement.textContent = 'Applying the visible camera settings…';
          await saveLabSettings();
          resultElement.textContent = actionPath === '/api/lab/capture' ? 'Capturing full-resolution test photo…' : 'Starting preview…';
        }
        const response = await fetch(actionPath, { method: 'POST' });
        const result = await response.json();
        if (!response.ok) throw new Error(result.message || 'HTTP ' + response.status);
        resultElement.textContent = result.message;
        if (actionPath === '/api/lab/capture') labCaptureLoaded = false;
      } catch (error) {
        resultElement.textContent = 'Request failed: ' + error.message;
      } finally {
        buttons.forEach(item => item.disabled = false);
        await refresh();
      }
    }
    document.querySelectorAll('button[data-action]').forEach(button => button.addEventListener('click', () => action(button)));
    byId('lab-settings').addEventListener('input', updateLabReadouts);
    byId('lab-settings').addEventListener('submit', async event => {
      event.preventDefault();
      const submit = event.submitter;
      submit.disabled = true;
      byId('lab-result').textContent = 'Applying camera settings…';
      try {
        byId('lab-result').textContent = await saveLabSettings();
      } catch (error) {
        byId('lab-result').textContent = 'Settings failed: ' + error.message;
      } finally {
        submit.disabled = false;
        await refresh();
      }
    });
    document.querySelectorAll('[data-focus-feet]').forEach(button => button.addEventListener('click', () => {
      const form = byId('lab-settings');
      const feet = Number(button.dataset.focusFeet);
      form.elements.namedItem('autofocus_mode').value = 'manual';
      form.elements.namedItem('lens_position').value = (3.28084 / feet).toFixed(2);
      updateLabReadouts();
      byId('lab-result').textContent = 'Fixed focus preset loaded for ' + feet + ' ft · start preview or take a test snap to apply it.';
    }));
    document.querySelectorAll('[data-focus-preset="portrait"]').forEach(button => button.addEventListener('click', () => {
      const form = byId('lab-settings');
      form.elements.namedItem('autofocus_mode').value = 'continuous';
      form.elements.namedItem('autofocus_range').value = 'normal';
      form.elements.namedItem('autofocus_speed').value = 'fast';
      form.elements.namedItem('autofocus_window_x').value = '0.2';
      form.elements.namedItem('autofocus_window_y').value = '0.15';
      form.elements.namedItem('autofocus_window_width').value = '0.6';
      form.elements.namedItem('autofocus_window_height').value = '0.7';
      updateLabReadouts();
      byId('lab-result').textContent = 'Portrait autofocus preset loaded · start preview or take a test snap to apply it.';
    }));
    byId('lab-reset').addEventListener('click', () => {
      populateLabSettings(labDefaults);
      byId('lab-result').textContent = 'Default controls restored · start preview or take a test snap to apply them.';
    });
    refresh();
    const events = new EventSource('/api/events');
    events.onmessage = event => render(JSON.parse(event.data));
    events.onerror = () => {
      byId('host').textContent = 'reconnecting';
    };
    setInterval(() => {
      const elapsed = Math.floor((Date.now() - uptimeReceivedAt) / 1000);
      byId('uptime').textContent = duration(uptimeBase + elapsed);
    }, 1000);
    setInterval(refreshLabFrame, 300);
    setInterval(refresh, 5000);
  </script>
</body>
</html>"#;

struct LedPanel {
    green: OutputPin,
    yellow: OutputPin,
    red: OutputPin,
    runtime_status: watch::Sender<RuntimeStatus>,
}

struct ProcessingPulse {
    step: u32,
}

impl LedPanel {
    fn new(
        gpio: &Gpio,
        config: &Config,
        runtime_status: watch::Sender<RuntimeStatus>,
    ) -> Result<Self> {
        let mut panel = Self {
            green: gpio.get(config.green_led_pin)?.into_output_low(),
            yellow: gpio.get(config.yellow_led_pin)?.into_output_low(),
            red: gpio.get(config.red_led_pin)?.into_output_low(),
            runtime_status,
        };
        panel.set(false, false, false);
        Ok(panel)
    }

    fn set(&mut self, green_on: bool, yellow_on: bool, red_on: bool) {
        let _ = self.green.clear_pwm();
        if green_on {
            self.green.set_high();
        } else {
            self.green.set_low();
        }
        if yellow_on {
            self.yellow.set_high();
        } else {
            self.yellow.set_low();
        }
        if red_on {
            self.red.set_high();
        } else {
            self.red.set_low();
        }
        set_led_status(&self.runtime_status, green_on, yellow_on, red_on);
    }

    fn ready(&mut self, has_queued_photos: bool) {
        self.set(true, false, has_queued_photos);
    }

    fn countdown(&mut self) {
        self.set(false, false, false);
        for count in 1..=3 {
            set_runtime_status(
                &self.runtime_status,
                "countdown",
                format!("{count} of 3 — get ready"),
            );
            self.set(false, true, false);
            thread::sleep(Duration::from_millis(650));
            self.set(false, false, false);
            thread::sleep(Duration::from_millis(350));
        }
        self.set(false, true, false);
    }

    fn begin_processing(&mut self) -> ProcessingPulse {
        self.set(true, false, false);
        let mut pulse = ProcessingPulse { step: 0 };
        self.processing_tick(&mut pulse);
        pulse
    }

    fn processing_tick(&mut self, pulse: &mut ProcessingPulse) {
        let duty_cycle = processing_duty_cycle(pulse.step);
        if self.green.set_pwm_frequency(100.0, duty_cycle).is_err() {
            self.green.set_high();
        }
        pulse.step = pulse.step.wrapping_add(1);
    }

    fn while_processing<T: Send>(
        &mut self,
        pulse: &mut ProcessingPulse,
        operation: impl FnOnce() -> T + Send,
    ) -> T {
        let result = thread::scope(|scope| {
            let (sender, receiver) = mpsc::sync_channel(1);
            scope.spawn(move || {
                let _ = sender.send(operation());
            });
            loop {
                match receiver.recv_timeout(Duration::from_millis(40)) {
                    Ok(result) => break result,
                    Err(mpsc::RecvTimeoutError::Timeout) => self.processing_tick(pulse),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        panic!("processing operation stopped without a result")
                    }
                }
            }
        });
        self.finish_processing();
        result
    }

    fn finish_processing(&mut self) {
        let _ = self.green.clear_pwm();
        self.set(false, false, false);
    }

    fn success(&mut self) {
        self.set(false, false, false);
        for _ in 0..2 {
            self.set(true, false, false);
            thread::sleep(Duration::from_millis(180));
            self.set(false, false, false);
            thread::sleep(Duration::from_millis(120));
        }
        self.ready(false);
    }

    fn error(&mut self) {
        self.set(false, false, true);
    }
}

fn processing_duty_cycle(step: u32) -> f64 {
    let phase = (step % 100) as f64 / 100.0 * std::f64::consts::TAU;
    0.12 + 0.58 * ((phase - std::f64::consts::FRAC_PI_2).sin() + 1.0) / 2.0
}

fn default_camera_args() -> Vec<String> {
    [
        "--nopreview",
        "--timeout",
        "3000",
        "--encoding",
        "jpg",
        "--quality",
        "95",
        "--autofocus-mode",
        "continuous",
        "--autofocus-range",
        "normal",
        "--autofocus-speed",
        "fast",
        "--autofocus-window",
        "0.2,0.15,0.6,0.7",
        "--metadata-format",
        "json",
        "--metadata",
        "-",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn capture_id() -> String {
    let suffix = Uuid::new_v4().simple().to_string();
    format!("{}-{}", Utc::now().format("%Y%m%dT%H%M%SZ"), &suffix[..8])
}

fn validate_jpeg_file(path: &Path) -> Result<()> {
    let mut file = File::open(path).with_context(|| format!("open JPEG {}", path.display()))?;
    let length = file.metadata()?.len();
    if length < 4 {
        bail!("{} is too small to be a JPEG", path.display());
    }
    let mut start = [0_u8; 2];
    let mut end = [0_u8; 2];
    file.read_exact(&mut start)?;
    file.seek(SeekFrom::End(-2))?;
    file.read_exact(&mut end)?;
    if start != [0xff, 0xd8] || end != [0xff, 0xd9] {
        bail!("{} is not a complete JPEG", path.display());
    }
    Ok(())
}

fn validate_jpeg_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        bail!("camera output is not a complete JPEG");
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn pressed_after_debounce(button: &InputPin) -> bool {
    thread::sleep(DEBOUNCE_INTERVAL);
    button.is_low()
}

fn wait_for_release(button: &InputPin) {
    while button.is_low() {
        thread::sleep(BUTTON_POLL_INTERVAL);
    }
    thread::sleep(DEBOUNCE_INTERVAL);
}

fn env_pin(name: &str, default: u8) -> Result<u8> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("{name} must be a valid BCM GPIO number"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn detect_camera_command() -> String {
    ["/usr/bin/rpicam-still", "/usr/bin/libcamera-still"]
        .into_iter()
        .find(|candidate| Path::new(candidate).exists())
        .unwrap_or("rpicam-still")
        .to_owned()
}

fn cpu_temperature_celsius() -> Option<f64> {
    fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|millidegrees| millidegrees / 1000.0)
}

fn load_average_one_minute() -> Option<f64> {
    fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn memory_usage() -> Option<(u64, u64)> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib = None;
    let mut available_kib = None;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("MemTotal:") => total_kib = fields.next()?.parse::<u64>().ok(),
            Some("MemAvailable:") => available_kib = fields.next()?.parse::<u64>().ok(),
            _ => {}
        }
    }
    let total = total_kib?.saturating_mul(1024);
    let available = available_kib?.saturating_mul(1024);
    Some((total.saturating_sub(available), total))
}

fn filesystem_usage(path: &Path) -> Option<(u64, u64, u64)> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let path = fs::canonicalize(path).ok()?;
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut statistics = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid NUL-terminated string and `statistics` points to
    // writable storage for one `statvfs` value.
    if unsafe { libc::statvfs(path.as_ptr(), statistics.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: a successful `statvfs` call initialized the complete structure.
    let statistics = unsafe { statistics.assume_init() };
    let block_size = statistics.f_frsize;
    let total = statistics.f_blocks.saturating_mul(block_size);
    let free = statistics.f_bfree.saturating_mul(block_size);
    let available = statistics.f_bavail.saturating_mul(block_size);
    Some((total.saturating_sub(free), total, available))
}

#[cfg(test)]
mod tests {
    use super::{
        ADMIN_PAGE, capture_id, default_camera_args, processing_duty_cycle, validate_jpeg_file,
    };
    use std::io::Write;

    #[test]
    fn capture_ids_are_safe_and_sortable() {
        let id = capture_id();
        assert_eq!(id.len(), 25);
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
    }

    #[test]
    fn validates_jpeg_boundaries() {
        let path = std::env::temp_dir().join(format!("{}.jpg", capture_id()));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&[0xff, 0xd8, 1, 2, 3, 0xff, 0xd9]).unwrap();
        drop(file);
        assert!(validate_jpeg_file(&path).is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn admin_page_uses_live_events_and_exposes_led_tests() {
        assert!(ADMIN_PAGE.contains("new EventSource('/api/events')"));
        assert!(ADMIN_PAGE.contains("/api/actions/led/green"));
        assert!(ADMIN_PAGE.contains("/api/actions/led/yellow"));
        assert!(ADMIN_PAGE.contains("/api/actions/led/red"));
        assert!(ADMIN_PAGE.contains("/api/actions/led/restore"));
        assert!(ADMIN_PAGE.contains("/api/lab/preview/start"));
        assert!(ADMIN_PAGE.contains("/api/lab/capture"));
        assert!(ADMIN_PAGE.contains("/api/lab/settings"));
        assert!(ADMIN_PAGE.contains("name=\"autofocus_mode\""));
        assert!(ADMIN_PAGE.contains("name=\"lens_position\""));
        assert!(ADMIN_PAGE.contains("name=\"autofocus_window_x\""));
        assert!(ADMIN_PAGE.contains("name=\"autofocus_window_height\""));
        assert!(ADMIN_PAGE.contains("data-focus-feet=\"3\""));
        assert!(ADMIN_PAGE.contains("id=\"focus-metadata-raw\""));
        assert!(ADMIN_PAGE.contains("id=\"lab-full-resolution\""));
        assert!(ADMIN_PAGE.contains("Ready / processing"));
        assert!(ADMIN_PAGE.contains("Countdown"));
    }

    #[test]
    fn default_capture_tracks_portrait_focus_during_the_three_second_countdown() {
        let args = default_camera_args();
        let timeout = args.iter().position(|arg| arg == "--timeout").unwrap();
        let autofocus = args
            .iter()
            .position(|arg| arg == "--autofocus-mode")
            .unwrap();

        assert_eq!(args.get(timeout + 1).map(String::as_str), Some("3000"));
        assert_eq!(
            args.get(autofocus + 1).map(String::as_str),
            Some("continuous")
        );
        let autofocus_speed = args
            .iter()
            .position(|arg| arg == "--autofocus-speed")
            .unwrap();
        assert_eq!(
            args.get(autofocus_speed + 1).map(String::as_str),
            Some("fast")
        );
        let autofocus_range = args
            .iter()
            .position(|arg| arg == "--autofocus-range")
            .unwrap();
        assert_eq!(
            args.get(autofocus_range + 1).map(String::as_str),
            Some("normal")
        );
        let autofocus_window = args
            .iter()
            .position(|arg| arg == "--autofocus-window")
            .unwrap();
        assert_eq!(
            args.get(autofocus_window + 1).map(String::as_str),
            Some("0.2,0.15,0.6,0.7")
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--metadata-format", "json"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--metadata", "-"]));
        assert!(!args.iter().any(|arg| arg == "--autofocus-on-capture"));
        assert!(!args.iter().any(|arg| arg == "--immediate"));
    }

    #[test]
    fn processing_green_breathes_within_a_safe_duty_cycle() {
        let duty_cycles = (0..100).map(processing_duty_cycle).collect::<Vec<_>>();
        let minimum = duty_cycles.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = duty_cycles
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        assert!((minimum - 0.12).abs() < 0.000_001);
        assert!((maximum - 0.70).abs() < 0.000_001);
        assert!((processing_duty_cycle(0) - processing_duty_cycle(100)).abs() < f64::EPSILON);
    }
}
