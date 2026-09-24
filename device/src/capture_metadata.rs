//! Per-photo capture metadata: what took the photograph, and with what
//! settings.
//!
//! The wire contract is `docs/capture-metadata.md` and `server/src/capture.rs`.
//! Every field is optional; the server ignores what it does not know and
//! merges `sensor_metadata` underneath anything stated explicitly here.
//!
//! Captures go through a durable queue and may be uploaded minutes or a reboot
//! later, so the metadata is written to a JSON sidecar beside the queued JPEG
//! at capture time and read back when the grant is requested. A queued photo
//! with no sidecar — one left by an older binary — simply uploads without a
//! `capture` object.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::camera_profile::CameraProfile;

/// What caused a capture, in the server's vocabulary (`button`, `debug`,
/// `schedule`, `app`). The physical button is `button`; the admin page's
/// "Capture now" is the app driving the device; `capture-once` is a debug run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTrigger {
    Button,
    Admin,
    Cli,
}

impl CaptureTrigger {
    pub fn name(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Admin => "app",
            Self::Cli => "debug",
        }
    }
}

/// What this camera reports about one capture. Mirrors `CaptureMetadata` in
/// `server/src/capture.rs`; unknown-to-the-server fields are ignored, so the
/// two sides may deploy in either order.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CaptureMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure_us: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analog_gain: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digital_gain: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub af_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_position: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour_temperature_k: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

impl CaptureMetadata {
    /// Describe a capture that has just been taken.
    ///
    /// `args` is the camera command line actually used, so the reported
    /// geometry and quality are what was asked for rather than what the
    /// profile would have asked for. When the operator's `DAILY_MIRROR_CAMERA_ARGS`
    /// leaves the geometry out, rpicam-still stills default to the full sensor
    /// frame, which is what the profile's resolution records.
    pub fn for_capture(
        profile: CameraProfile,
        args: &[String],
        trigger: CaptureTrigger,
        sensor_metadata: Option<&Value>,
    ) -> Self {
        let (default_width, default_height) = profile.resolution();
        let mut capture = Self {
            firmware_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            sensor: Some(profile.name().to_owned()),
            width: flag(args, "--width").or(Some(i64::from(default_width))),
            height: flag(args, "--height").or(Some(i64::from(default_height))),
            jpeg_quality: flag(args, "--quality"),
            trigger: Some(trigger.name().to_owned()),
            capture_source: Some("device".to_owned()),
            // The grant endpoint refuses offsets other than `Z`.
            captured_at: Some(Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            ..Self::default()
        };
        if let Some(metadata) = sensor_metadata {
            capture.merge_sensor_metadata(metadata);
        }
        capture
    }

    /// Map the libcamera control block onto the documented fields. The server
    /// performs the same merge from `sensor_metadata`; doing it here too means
    /// the values survive even if the raw block is ever dropped.
    fn merge_sensor_metadata(&mut self, metadata: &Value) {
        self.exposure_us = self
            .exposure_us
            .or_else(|| integer(metadata, "ExposureTime"));
        self.analog_gain = self.analog_gain.or_else(|| real(metadata, "AnalogueGain"));
        self.digital_gain = self.digital_gain.or_else(|| real(metadata, "DigitalGain"));
        self.lens_position = self
            .lens_position
            .or_else(|| real(metadata, "LensPosition"));
        self.colour_temperature_k = self
            .colour_temperature_k
            .or_else(|| integer(metadata, "ColourTemperature"));
        self.af_state = self
            .af_state
            .take()
            .or_else(|| af_state(metadata.get("AfState")));
    }
}

/// libcamera prints `AfState` as an integer enum in some builds and a name in
/// others. Normalise both onto the server's four names; anything unrecognised
/// is dropped rather than risking a `400` on the whole upload.
fn af_state(value: Option<&Value>) -> Option<String> {
    let raw = match value? {
        Value::String(name) => name.clone(),
        Value::Number(code) => code.to_string(),
        _ => return None,
    };
    let name = match raw.trim().to_ascii_lowercase().as_str() {
        "0" | "idle" | "afstateidle" => "unknown",
        "1" | "scanning" | "searching" | "afstatescanning" => "searching",
        "2" | "focused" | "afstatefocused" => "focused",
        "3" | "failed" | "afstatefailed" => "failed",
        _ => return None,
    };
    Some(name.to_owned())
}

fn integer(metadata: &Value, key: &str) -> Option<i64> {
    let value = metadata.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number.round() as i64))
}

fn real(metadata: &Value, key: &str) -> Option<f64> {
    metadata.get(key)?.as_f64()
}

/// Read the value following a camera command-line flag, in either
/// `--flag value` or `--flag=value` form.
fn flag(args: &[String], name: &str) -> Option<i64> {
    let prefix = format!("{name}=");
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .or_else(|| {
            args.iter()
                .find_map(|arg| arg.strip_prefix(prefix.as_str()))
        })
        .and_then(|value| value.parse().ok())
}

/// The JSON written beside a queued JPEG, and the shape of the extra fields on
/// the upload-grant request body.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CaptureSidecar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureMetadata>,
    /// The raw libcamera control block, forwarded verbatim. The server merges
    /// it underneath `capture`, so it costs nothing and carries controls this
    /// firmware does not map yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor_metadata: Option<Value>,
}

impl CaptureSidecar {
    /// Where a queued photo's sidecar lives: `<capture-id>.json` beside
    /// `<capture-id>.jpg`, matching the sidecars local captures already write.
    pub fn path_for(photo: &Path) -> PathBuf {
        photo.with_extension("json")
    }

    /// Best effort: a photo queued by an older binary has no sidecar, and a
    /// corrupt one must not strand a photograph in the queue forever.
    pub fn read(photo: &Path) -> Option<Self> {
        let bytes = std::fs::read(Self::path_for(photo)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn libcamera_block() -> Value {
        serde_json::json!({
            "ExposureTime": 19994,
            "AnalogueGain": 8.0,
            "DigitalGain": 1.02,
            "ColourTemperature": 2800,
            "LensPosition": 3.5,
            "AfState": 2,
            "FrameDuration": 33333,
        })
    }

    #[test]
    fn describes_a_capture_from_the_profile_and_the_libcamera_block() {
        let args = CameraProfile::Imx519.capture_args();
        let capture = CaptureMetadata::for_capture(
            CameraProfile::Imx519,
            &args,
            CaptureTrigger::Button,
            Some(&libcamera_block()),
        );
        assert_eq!(capture.sensor.as_deref(), Some("imx519"));
        assert_eq!(
            capture.firmware_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(capture.width, Some(4656));
        assert_eq!(capture.height, Some(3496));
        assert_eq!(capture.jpeg_quality, Some(95));
        assert_eq!(capture.exposure_us, Some(19994));
        assert_eq!(capture.analog_gain, Some(8.0));
        assert_eq!(capture.digital_gain, Some(1.02));
        assert_eq!(capture.lens_position, Some(3.5));
        assert_eq!(capture.colour_temperature_k, Some(2800));
        assert_eq!(capture.af_state.as_deref(), Some("focused"));
        assert_eq!(capture.trigger.as_deref(), Some("button"));
        assert_eq!(capture.capture_source.as_deref(), Some("device"));
        let captured_at = capture.captured_at.unwrap();
        assert!(captured_at.ends_with('Z'), "{captured_at}");
        assert_eq!(captured_at.len(), 20);
    }

    #[test]
    fn falls_back_to_the_profile_geometry_and_tolerates_a_missing_block() {
        let capture = CaptureMetadata::for_capture(
            CameraProfile::Ov5647,
            &["--nopreview".to_owned()],
            CaptureTrigger::Cli,
            None,
        );
        assert_eq!((capture.width, capture.height), (Some(2592), Some(1944)));
        assert_eq!(capture.jpeg_quality, None);
        assert_eq!(capture.exposure_us, None);
        assert_eq!(capture.af_state, None);
        assert_eq!(capture.trigger.as_deref(), Some("debug"));
        assert_eq!(
            CaptureTrigger::Admin.name(),
            "app",
            "the admin page's Capture now is the app driving the device"
        );
    }

    #[test]
    fn reads_flags_written_with_an_equals_sign_and_named_af_states() {
        let args = ["--width=1280".to_owned(), "--quality=70".to_owned()];
        let capture = CaptureMetadata::for_capture(
            CameraProfile::Imx219,
            &args,
            CaptureTrigger::Admin,
            Some(&serde_json::json!({ "AfState": "AfStateScanning" })),
        );
        assert_eq!(capture.width, Some(1280));
        assert_eq!(capture.jpeg_quality, Some(70));
        assert_eq!(capture.af_state.as_deref(), Some("searching"));
        assert_eq!(af_state(Some(&serde_json::json!("bogus"))), None);
    }

    #[test]
    fn a_sidecar_round_trips_beside_its_photo() {
        let sidecar = CaptureSidecar {
            capture: Some(CaptureMetadata::for_capture(
                CameraProfile::Imx519,
                &CameraProfile::Imx519.capture_args(),
                CaptureTrigger::Button,
                Some(&libcamera_block()),
            )),
            sensor_metadata: Some(libcamera_block()),
        };
        let root = std::env::temp_dir().join(format!("capture-sidecar-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let photo = root.join("20260919T140311Z-1a2b3c4d.jpg");
        std::fs::write(&photo, b"jpeg").unwrap();
        assert_eq!(
            CaptureSidecar::path_for(&photo),
            root.join("20260919T140311Z-1a2b3c4d.json")
        );
        assert!(
            CaptureSidecar::read(&photo).is_none(),
            "a photo queued by an older binary has no sidecar"
        );
        std::fs::write(
            CaptureSidecar::path_for(&photo),
            serde_json::to_vec(&sidecar).unwrap(),
        )
        .unwrap();
        assert_eq!(CaptureSidecar::read(&photo).unwrap(), sidecar);

        // A corrupt sidecar must not strand the photograph in the queue.
        std::fs::write(CaptureSidecar::path_for(&photo), b"{ not json").unwrap();
        assert!(CaptureSidecar::read(&photo).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
