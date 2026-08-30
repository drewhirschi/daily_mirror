use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const PREVIEW_WIDTH: &str = "960";
const PREVIEW_HEIGHT: &str = "720";
const PREVIEW_FRAMERATE: &str = "8";
const PREVIEW_SENSOR_MODE: &str = "2328:1748:10:P";
const PREVIEW_QUALITY: &str = "90";
const MAX_PREVIEW_JPEG_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CameraOrientation {
    pub rotation_degrees: u16,
    pub hflip: bool,
    pub vflip: bool,
}

impl CameraOrientation {
    pub fn validate(&self) -> Result<()> {
        if matches!(self.rotation_degrees, 0 | 180) {
            Ok(())
        } else {
            bail!("camera rotation must be 0 or 180 degrees")
        }
    }

    pub fn camera_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.rotation_degrees != 0 {
            args.extend(["--rotation".to_owned(), self.rotation_degrees.to_string()]);
        }
        if self.hflip {
            args.push("--hflip".to_owned());
        }
        if self.vflip {
            args.push("--vflip".to_owned());
        }
        args
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LabSettings {
    pub rotation_degrees: u16,
    pub hflip: bool,
    pub vflip: bool,
    pub ev: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub sharpness: f64,
    pub denoise: String,
    pub awb: String,
    pub metering: String,
    pub exposure: String,
    pub shutter_us: u64,
    pub gain: f64,
    pub autofocus_range: String,
    pub autofocus_speed: String,
}

impl Default for LabSettings {
    fn default() -> Self {
        Self {
            rotation_degrees: 0,
            hflip: false,
            vflip: false,
            ev: 0.0,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            sharpness: 1.0,
            denoise: "auto".to_owned(),
            awb: "auto".to_owned(),
            metering: "centre".to_owned(),
            exposure: "normal".to_owned(),
            shutter_us: 0,
            gain: 0.0,
            autofocus_range: "normal".to_owned(),
            autofocus_speed: "normal".to_owned(),
        }
    }
}

impl LabSettings {
    pub fn with_orientation(mut self, orientation: &CameraOrientation) -> Self {
        self.rotation_degrees = orientation.rotation_degrees;
        self.hflip = orientation.hflip;
        self.vflip = orientation.vflip;
        self
    }

    pub fn orientation(&self) -> CameraOrientation {
        CameraOrientation {
            rotation_degrees: self.rotation_degrees,
            hflip: self.hflip,
            vflip: self.vflip,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.orientation().validate()?;
        validate_number("EV", self.ev, -4.0, 4.0)?;
        validate_number("brightness", self.brightness, -0.5, 0.5)?;
        validate_number("contrast", self.contrast, 0.0, 3.0)?;
        validate_number("saturation", self.saturation, 0.0, 3.0)?;
        validate_number("sharpness", self.sharpness, 0.0, 4.0)?;
        validate_number("gain", self.gain, 0.0, 16.0)?;
        if self.shutter_us > 1_000_000 {
            bail!("shutter must be automatic or at most 1,000,000 microseconds");
        }
        validate_choice(
            "denoise",
            &self.denoise,
            &["auto", "off", "cdn_off", "cdn_fast", "cdn_hq"],
        )?;
        validate_choice(
            "white balance",
            &self.awb,
            &[
                "auto",
                "incandescent",
                "tungsten",
                "fluorescent",
                "indoor",
                "daylight",
                "cloudy",
            ],
        )?;
        validate_choice("metering", &self.metering, &["centre", "average", "spot"])?;
        validate_choice("exposure", &self.exposure, &["normal", "sport"])?;
        validate_choice(
            "autofocus range",
            &self.autofocus_range,
            &["normal", "macro", "full"],
        )?;
        validate_choice(
            "autofocus speed",
            &self.autofocus_speed,
            &["normal", "fast"],
        )?;
        Ok(())
    }

    pub fn camera_args(&self) -> Vec<String> {
        let mut args = vec![
            "--ev".to_owned(),
            self.ev.to_string(),
            "--brightness".to_owned(),
            self.brightness.to_string(),
            "--contrast".to_owned(),
            self.contrast.to_string(),
            "--saturation".to_owned(),
            self.saturation.to_string(),
            "--sharpness".to_owned(),
            self.sharpness.to_string(),
            "--denoise".to_owned(),
            self.denoise.clone(),
            "--awb".to_owned(),
            self.awb.clone(),
            "--metering".to_owned(),
            self.metering.clone(),
            "--exposure".to_owned(),
            self.exposure.clone(),
            "--autofocus-range".to_owned(),
            self.autofocus_range.clone(),
            "--autofocus-speed".to_owned(),
            self.autofocus_speed.clone(),
        ];
        if self.shutter_us > 0 {
            args.push("--shutter".to_owned());
            args.push(self.shutter_us.to_string());
        }
        if self.gain > 0.0 {
            args.push("--gain".to_owned());
            args.push(self.gain.to_string());
        }
        args.extend(self.orientation().camera_args());
        args
    }
}

pub fn load_orientation(path: &Path) -> Result<CameraOrientation> {
    if !path.exists() {
        return Ok(CameraOrientation::default());
    }
    let bytes = fs::read(path)
        .with_context(|| format!("read camera orientation from {}", path.display()))?;
    let orientation: CameraOrientation = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse camera orientation from {}", path.display()))?;
    orientation.validate()?;
    Ok(orientation)
}

fn save_orientation(path: &Path, orientation: &CameraOrientation) -> Result<()> {
    orientation.validate()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create camera settings directory {}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(orientation).context("serialize camera orientation")?;
    let mut file = File::create(&temporary)
        .with_context(|| format!("create temporary camera settings {}", temporary.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write temporary camera settings {}", temporary.display()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "commit camera settings from {} to {}",
            temporary.display(),
            path.display()
        )
    })?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Clone)]
pub struct CameraLab {
    camera_lock: Arc<Mutex<()>>,
    orientation: Arc<RwLock<CameraOrientation>>,
    orientation_path: PathBuf,
    preview_command: String,
    settings: Arc<Mutex<LabSettings>>,
    preview: Arc<PreviewState>,
    capture: Arc<RwLock<Option<Vec<u8>>>>,
}

struct PreviewState {
    running: AtomicBool,
    stop_requested: AtomicBool,
    child_id: Mutex<Option<u32>>,
    frame: RwLock<Option<Vec<u8>>>,
    last_error: Mutex<Option<String>>,
}

impl CameraLab {
    pub fn new(
        camera_lock: Arc<Mutex<()>>,
        orientation: Arc<RwLock<CameraOrientation>>,
        orientation_path: PathBuf,
    ) -> Self {
        let initial_orientation = orientation
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Self {
            camera_lock,
            orientation,
            orientation_path,
            preview_command: detect_preview_command(),
            settings: Arc::new(Mutex::new(
                LabSettings::default().with_orientation(&initial_orientation),
            )),
            preview: Arc::new(PreviewState {
                running: AtomicBool::new(false),
                stop_requested: AtomicBool::new(false),
                child_id: Mutex::new(None),
                frame: RwLock::new(None),
                last_error: Mutex::new(None),
            }),
            capture: Arc::new(RwLock::new(None)),
        }
    }

    pub fn settings(&self) -> LabSettings {
        self.settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn apply_settings(&self, settings: LabSettings) -> Result<String> {
        settings.validate()?;
        let orientation = settings.orientation();
        let restart = self.preview_running();
        if restart {
            self.stop_preview()?;
        }
        save_orientation(&self.orientation_path, &orientation)?;
        *self
            .orientation
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = orientation;
        *self
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = settings;
        *self
            .preview
            .frame
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        if restart {
            self.start_preview()?;
            Ok("Settings applied and orientation saved; live preview restarting".to_owned())
        } else {
            Ok("Settings applied and orientation saved".to_owned())
        }
    }

    pub fn start_preview(&self) -> Result<String> {
        if self.preview.running.swap(true, Ordering::SeqCst) {
            return Ok("Live preview is already running".to_owned());
        }
        self.preview.stop_requested.store(false, Ordering::SeqCst);
        *self
            .preview
            .last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;

        let camera_lock = Arc::clone(&self.camera_lock);
        let preview = Arc::clone(&self.preview);
        let command = self.preview_command.clone();
        let settings = self.settings();
        let spawn_result = thread::Builder::new()
            .name("daily-mirror-preview".to_owned())
            .spawn(move || {
                let result = run_preview(command, camera_lock, Arc::clone(&preview), settings);
                if let Err(error) = result {
                    *preview
                        .last_error
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                        Some(format!("{error:#}"));
                }
                *preview
                    .child_id
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                preview.running.store(false, Ordering::SeqCst);
            });
        if let Err(error) = spawn_result {
            self.preview.running.store(false, Ordering::SeqCst);
            return Err(error.into());
        }
        Ok("Live preview starting".to_owned())
    }

    pub fn stop_preview(&self) -> Result<String> {
        if !self.preview_running() {
            return Ok("Live preview is already stopped".to_owned());
        }
        self.preview.stop_requested.store(true, Ordering::SeqCst);
        signal_preview_child(&self.preview, libc::SIGTERM);
        for _ in 0..100 {
            if !self.preview_running() {
                return Ok("Live preview stopped".to_owned());
            }
            thread::sleep(Duration::from_millis(20));
        }
        signal_preview_child(&self.preview, libc::SIGKILL);
        for _ in 0..50 {
            if !self.preview_running() {
                return Ok("Live preview stopped".to_owned());
            }
            thread::sleep(Duration::from_millis(20));
        }
        bail!("preview process did not stop in time")
    }

    pub fn preview_running(&self) -> bool {
        self.preview.running.load(Ordering::SeqCst)
    }

    pub fn preview_frame(&self) -> Option<Vec<u8>> {
        self.preview
            .frame
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn preview_error(&self) -> Option<String> {
        self.preview
            .last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn store_capture(&self, jpeg: Vec<u8>) {
        *self
            .capture
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(jpeg);
    }

    pub fn capture(&self) -> Option<Vec<u8>> {
        self.capture
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn has_capture(&self) -> bool {
        self.capture
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }
}

fn run_preview(
    command: String,
    camera_lock: Arc<Mutex<()>>,
    preview: Arc<PreviewState>,
    settings: LabSettings,
) -> Result<()> {
    let _camera = camera_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if preview.stop_requested.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mut child = Command::new(&command)
        .args([
            "--nopreview",
            "--timeout",
            "0",
            "--codec",
            "mjpeg",
            "--mode",
            PREVIEW_SENSOR_MODE,
            "--width",
            PREVIEW_WIDTH,
            "--height",
            PREVIEW_HEIGHT,
            "--framerate",
            PREVIEW_FRAMERATE,
            "--quality",
            PREVIEW_QUALITY,
            "--flush",
            "--autofocus-mode",
            "continuous",
            "--output",
            "-",
        ])
        .args(settings.camera_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start live preview with {command}"))?;
    *preview
        .child_id
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child.id());

    let stdout = child
        .stdout
        .take()
        .context("live preview did not provide an image stream")?;
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let mut buffer = [0_u8; 64 * 1024];
    let mut current = Vec::with_capacity(512 * 1024);
    let mut inside_jpeg = false;
    let mut previous_was_ff = false;

    loop {
        if preview.stop_requested.load(Ordering::SeqCst) {
            let _ = child.kill();
            break;
        }
        let bytes_read = reader
            .read(&mut buffer)
            .context("read live preview frame")?;
        if bytes_read == 0 {
            break;
        }
        for &byte in &buffer[..bytes_read] {
            if !inside_jpeg {
                if previous_was_ff && byte == 0xd8 {
                    current.clear();
                    current.extend_from_slice(&[0xff, 0xd8]);
                    inside_jpeg = true;
                    previous_was_ff = false;
                } else {
                    previous_was_ff = byte == 0xff;
                }
                continue;
            }

            current.push(byte);
            let length = current.len();
            if length >= 2 && current[length - 2..] == [0xff, 0xd9] {
                *preview
                    .frame
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(current.clone());
                current.clear();
                inside_jpeg = false;
                previous_was_ff = false;
            } else if length > MAX_PREVIEW_JPEG_BYTES {
                current.clear();
                inside_jpeg = false;
                previous_was_ff = false;
            }
        }
    }

    let status = child.wait().context("wait for live preview process")?;
    if !preview.stop_requested.load(Ordering::SeqCst) && !status.success() {
        bail!("live preview exited with {status}");
    }
    Ok(())
}

fn signal_preview_child(preview: &PreviewState, signal: libc::c_int) {
    let child_id = *preview
        .child_id
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(child_id) = child_id {
        // SAFETY: the PID belongs to the preview child started by this process.
        unsafe {
            libc::kill(child_id as libc::pid_t, signal);
        }
    }
}

fn detect_preview_command() -> String {
    ["/usr/bin/rpicam-vid", "/usr/bin/libcamera-vid"]
        .into_iter()
        .find(|candidate| Path::new(candidate).exists())
        .unwrap_or("rpicam-vid")
        .to_owned()
}

fn validate_number(name: &str, value: f64, minimum: f64, maximum: f64) -> Result<()> {
    if value.is_finite() && (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        bail!("{name} must be between {minimum} and {maximum}")
    }
}

fn validate_choice(name: &str, value: &str, choices: &[&str]) -> Result<()> {
    if choices.contains(&value) {
        Ok(())
    } else {
        bail!("unsupported {name}: {value}")
    }
}

#[cfg(test)]
mod tests {
    use super::{CameraOrientation, LabSettings, load_orientation, save_orientation};

    #[test]
    fn validates_safe_lab_settings_and_omits_automatic_manual_controls() {
        let settings = LabSettings::default();
        settings.validate().unwrap();
        let args = settings.camera_args();
        assert!(!args.iter().any(|arg| arg == "--shutter"));
        assert!(!args.iter().any(|arg| arg == "--gain"));
    }

    #[test]
    fn rejects_out_of_range_lab_settings() {
        let settings = LabSettings {
            gain: 100.0,
            ..LabSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn orientation_args_are_applied_to_camera_commands() {
        let orientation = CameraOrientation {
            rotation_degrees: 180,
            hflip: true,
            vflip: false,
        };
        assert_eq!(
            orientation.camera_args(),
            ["--rotation", "180", "--hflip"].map(str::to_owned)
        );
    }

    #[test]
    fn orientation_survives_a_settings_file_round_trip() {
        let directory =
            std::env::temp_dir().join(format!("daily-mirror-orientation-{}", std::process::id()));
        let path = directory.join("camera-settings.json");
        let orientation = CameraOrientation {
            rotation_degrees: 180,
            hflip: false,
            vflip: true,
        };
        save_orientation(&path, &orientation).unwrap();
        assert_eq!(load_orientation(&path).unwrap(), orientation);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
