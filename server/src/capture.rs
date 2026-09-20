//! Per-photo capture metadata: what took the photograph, and with what
//! settings.
//!
//! Every field is optional, because each sensor reports what it can and
//! photographs taken before this release report nothing. Units and the exact
//! JSON a camera should send are documented in `docs/capture-metadata.md`,
//! which is the contract the firmware implements against.
//!
//! Unknown fields are ignored rather than rejected, so firmware and server can
//! be deployed in either order.

use std::io;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The longest a free-text identifier may be before it is refused. These end
/// up on every photo row and in the mobile detail view.
const MAX_TEXT: usize = 64;

/// Capture IDs encode their own timestamp; anything before this is either a
/// clock that never synchronised or a malformed ID.
const MIN_YEAR: i64 = 2020;

/// Focus state reported by the lens driver.
const AF_STATES: &[&str] = &["unknown", "searching", "focused", "failed"];
/// What caused the capture.
const TRIGGERS: &[&str] = &["button", "debug", "schedule", "app"];
/// Which kind of client produced the photograph.
const CAPTURE_SOURCES: &[&str] = &["device", "phone", "legacy"];

/// What a camera reports about one capture.
///
/// See `docs/capture-metadata.md` for units. In short: times are microseconds,
/// gains are dimensionless multipliers, `lens_position` is dioptres, and
/// `mean_luma` is the sensor's average luminance on a 0-255 scale.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct CaptureMetadata {
    /// Software version of the camera that took this photograph. When a
    /// paired device omits it, the server snapshots `devices.firmware_version`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    /// Image sensor part, lowercase: `ov5640`, `imx519`, `imx708`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    /// Encoded pixel dimensions, before any server-side rotation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    /// JPEG quality normalised to 0-100 on every platform.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<i64>,
    /// Exposure time in microseconds (libcamera `ExposureTime`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_us: Option<i64>,
    /// Analogue sensor gain as a dimensionless multiplier (1.0 = unity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analog_gain: Option<f64>,
    /// Digital gain as a dimensionless multiplier. Pi only today.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digital_gain: Option<f64>,
    /// `unknown`, `searching`, `focused` or `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub af_state: Option<String>,
    /// Lens position in dioptres (reciprocal metres). Pi/IMX519 only today.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_position: Option<f64>,
    /// Estimated scene colour temperature in kelvin. Pi only today.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour_temperature_k: Option<i64>,
    /// Sensor average luminance, 0-255. Low light is this project's known
    /// cause of blur, so this is the field to sort a bad batch by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_luma: Option<i64>,
    /// Autofocus sharpness score, when the AF firmware reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_score: Option<i64>,
    /// `button`, `debug`, `schedule` or `app`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// `device`, `phone` or `legacy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_source: Option<String>,
    /// When the shutter fired, RFC 3339 in UTC. Preferred over the timestamp
    /// embedded in the capture ID when it is present and plausible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

impl CaptureMetadata {
    /// Reject absurd values and cap free text.
    ///
    /// A camera that reports a nonsense number is a camera worth noticing, so
    /// this refuses the upload rather than storing a value Drew would later
    /// have to distrust. The one exception is text, which is trimmed and
    /// length-checked rather than parsed.
    pub fn validated(mut self) -> io::Result<Self> {
        self.firmware_version = text(self.firmware_version, "firmware_version")?;
        self.sensor = text(self.sensor, "sensor")?.map(|value| value.to_lowercase());
        self.width = range(self.width, 1, 20_000, "width")?;
        self.height = range(self.height, 1, 20_000, "height")?;
        self.jpeg_quality = range(self.jpeg_quality, 0, 100, "jpeg_quality")?;
        self.exposure_us = range(self.exposure_us, 1, 60_000_000, "exposure_us")?;
        self.analog_gain = real(self.analog_gain, 0.0, 1024.0, "analog_gain")?;
        self.digital_gain = real(self.digital_gain, 0.0, 1024.0, "digital_gain")?;
        self.lens_position = real(self.lens_position, -10.0, 100.0, "lens_position")?;
        self.colour_temperature_k = range(
            self.colour_temperature_k,
            500,
            30_000,
            "colour_temperature_k",
        )?;
        self.mean_luma = range(self.mean_luma, 0, 255, "mean_luma")?;
        self.focus_score = range(self.focus_score, 0, 1_000_000, "focus_score")?;
        self.af_state = one_of(self.af_state, AF_STATES, "af_state")?;
        self.trigger = one_of(self.trigger, TRIGGERS, "trigger")?;
        self.capture_source = one_of(self.capture_source, CAPTURE_SOURCES, "capture_source")?;
        self.captured_at = match self.captured_at.take() {
            Some(value) => plausible_timestamp(&value)?,
            None => None,
        };
        Ok(self)
    }

    /// Fill `firmware_version` from the paired device's record when the camera
    /// did not send one, so every device photo carries the version that took
    /// it even before the firmware learns to report it.
    pub fn with_device_firmware(mut self, firmware_version: Option<&str>) -> Self {
        if self.firmware_version.is_none()
            && let Some(version) = firmware_version
        {
            let trimmed = version.trim();
            if !trimmed.is_empty() {
                self.firmware_version = Some(truncate(trimmed));
            }
        }
        self
    }

    /// Default `capture_source` when the client did not say.
    pub fn with_source(mut self, source: &str) -> Self {
        if self.capture_source.is_none() {
            self.capture_source = Some(source.to_owned());
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// What the gallery shows about one photograph's provenance: the same fields
/// the camera reported, plus the paired camera's name so a bad photograph
/// names the mirror it came from.
///
/// Flat rather than nested so the response reads exactly like the request.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct PhotoCapture {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// The name the household gave this camera when it was paired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jpeg_quality: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_us: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analog_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digital_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub af_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour_temperature_k: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_luma: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_score: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_source: Option<String>,
}

impl PhotoCapture {
    /// True when nothing at all is known, so the gallery can hide the block
    /// instead of showing an empty one.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The libcamera control names `rpicam-still --metadata-format json` prints,
/// which the Pi already writes beside each capture. Accepting this shape lets
/// the Pi forward its sidecar unchanged; explicit `capture` fields win.
///
/// Unknown controls are ignored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, ToSchema)]
pub struct SensorMetadata {
    #[serde(rename = "ExposureTime", skip_serializing_if = "Option::is_none")]
    pub exposure_time: Option<i64>,
    #[serde(rename = "AnalogueGain", skip_serializing_if = "Option::is_none")]
    pub analogue_gain: Option<f64>,
    #[serde(rename = "DigitalGain", skip_serializing_if = "Option::is_none")]
    pub digital_gain: Option<f64>,
    #[serde(rename = "ColourTemperature", skip_serializing_if = "Option::is_none")]
    pub colour_temperature: Option<i64>,
    #[serde(rename = "LensPosition", skip_serializing_if = "Option::is_none")]
    pub lens_position: Option<f64>,
    /// libcamera prints this as an integer enum in some builds and a name in
    /// others, so it is read loosely and normalised below.
    #[serde(rename = "AfState", skip_serializing_if = "Option::is_none")]
    pub af_state: Option<serde_json::Value>,
}

impl SensorMetadata {
    /// Both the integer enum and the name map onto this server's vocabulary.
    fn af_state(&self) -> Option<String> {
        let raw = match self.af_state.as_ref()? {
            serde_json::Value::String(name) => name.clone(),
            serde_json::Value::Number(code) => code.to_string(),
            _ => return None,
        };
        let raw = raw.trim().to_ascii_lowercase();
        Some(
            match raw.as_str() {
                "0" | "idle" | "afstateidle" => "unknown",
                "1" | "scanning" | "searching" | "afstatescanning" => "searching",
                "2" | "focused" | "afstatefocused" => "focused",
                "3" | "failed" | "afstatefailed" => "failed",
                other => other,
            }
            .to_owned(),
        )
    }

    /// Merge into `capture`, leaving anything the client stated explicitly.
    pub fn merge_into(&self, mut capture: CaptureMetadata) -> CaptureMetadata {
        capture.exposure_us = capture.exposure_us.or(self.exposure_time);
        capture.analog_gain = capture.analog_gain.or(self.analogue_gain);
        capture.digital_gain = capture.digital_gain.or(self.digital_gain);
        capture.colour_temperature_k = capture.colour_temperature_k.or(self.colour_temperature);
        capture.lens_position = capture.lens_position.or(self.lens_position);
        capture.af_state = capture.af_state.or_else(|| self.af_state());
        capture
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn truncate(value: &str) -> String {
    value.chars().take(MAX_TEXT).collect()
}

fn text(value: Option<String>, field: &str) -> io::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_TEXT {
        return Err(invalid(format!(
            "capture.{field} must be at most {MAX_TEXT} characters"
        )));
    }
    if trimmed.chars().any(|character| character.is_control()) {
        return Err(invalid(format!(
            "capture.{field} must not contain control characters"
        )));
    }
    Ok(Some(trimmed.to_owned()))
}

fn range(value: Option<i64>, low: i64, high: i64, field: &str) -> io::Result<Option<i64>> {
    match value {
        Some(number) if !(low..=high).contains(&number) => Err(invalid(format!(
            "capture.{field} must be between {low} and {high}"
        ))),
        other => Ok(other),
    }
}

fn real(value: Option<f64>, low: f64, high: f64, field: &str) -> io::Result<Option<f64>> {
    match value {
        Some(number) if !number.is_finite() || number < low || number > high => Err(invalid(
            format!("capture.{field} must be between {low} and {high}"),
        )),
        other => Ok(other),
    }
}

fn one_of(value: Option<String>, allowed: &[&str], field: &str) -> io::Result<Option<String>> {
    let Some(value) = text(value, field)? else {
        return Ok(None);
    };
    let lowered = value.to_ascii_lowercase();
    if allowed.contains(&lowered.as_str()) {
        Ok(Some(lowered))
    } else {
        Err(invalid(format!(
            "capture.{field} must be one of {}",
            allowed.join(", ")
        )))
    }
}

/// Parse an RFC 3339 UTC instant and refuse one that cannot be real.
///
/// A camera whose clock never synchronised reports 1970, and a camera whose
/// clock ran away reports 2106; both used to become `captured_at` values that
/// sorted above every real photograph. Out-of-range values are dropped rather
/// than refused, so a bad clock does not cost the photograph — the ID-derived
/// timestamp is used instead.
pub fn plausible_timestamp(value: &str) -> io::Result<Option<String>> {
    let Some(parsed) = parse_rfc3339_utc(value.trim()) else {
        return Err(invalid(
            "capture.captured_at must be an RFC 3339 UTC timestamp such as 2026-09-19T14:03:11Z"
                .to_owned(),
        ));
    };
    let (year, ..) = parsed;
    let (max_year, ..) = civil_from_unix(now_seconds() + 24 * 60 * 60);
    if year < MIN_YEAR || year > max_year {
        return Ok(None);
    }
    Ok(Some(format_civil(parsed)))
}

type Civil = (i64, u32, u32, u32, u32, u32);

fn format_civil((year, month, day, hour, minute, second): Civil) -> String {
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Strict RFC 3339 in UTC: `YYYY-MM-DDTHH:MM:SS[.fff]Z`. Offsets other than
/// `Z` are refused so no timezone arithmetic is ever needed here.
fn parse_rfc3339_utc(value: &str) -> Option<Civil> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !matches!(bytes[10], b'T' | b't' | b' ') || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let tail = &value[19..];
    let tail = tail.strip_prefix('.').map_or(tail, |fraction| {
        let digits = fraction.chars().take_while(char::is_ascii_digit).count();
        &fraction[digits..]
    });
    if !matches!(tail, "Z" | "z" | "+00:00" | "-00:00") {
        return None;
    }
    let year = value[0..4].parse().ok()?;
    let month = value[5..7].parse().ok()?;
    let day = value[8..10].parse().ok()?;
    let hour = value[11..13].parse().ok()?;
    let minute = value[14..16].parse().ok()?;
    let second = value[17..19].parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    Some((year, month, day, hour, minute, second))
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Howard Hinnant's `civil_from_days`, which needs no calendar crate.
fn civil_from_unix(seconds: i64) -> Civil {
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (
        year + i64::from(month <= 2),
        month,
        day,
        (remainder / 3_600) as u32,
        (remainder % 3_600 / 60) as u32,
        (remainder % 60) as u32,
    )
}

/// The capture ID convention every client shares: `YYYYMMDDTHHMMSSZ-<8 hex>`.
///
/// The catalog derives `photos.captured_at` by slicing this prefix, so an ID
/// in any other shape stored a timestamp that sorted above every real
/// photograph — and, because reservation upserts on the ID, two captures from
/// a camera with a repeating suffix could overwrite one another.
pub fn validate_capture_id_format(id: &str) -> io::Result<()> {
    let refuse = || {
        Err(invalid(
            "capture_id must look like 20260919T140311Z-1a2b3c4d: an RFC 3339 UTC instant \
             without separators, a dash, then 8 lowercase hex characters"
                .to_owned(),
        ))
    };
    let bytes = id.as_bytes();
    if bytes.len() != 25 || bytes[8] != b'T' || bytes[15] != b'Z' || bytes[16] != b'-' {
        return refuse();
    }
    if !id[0..8]
        .bytes()
        .chain(id[9..15].bytes())
        .all(|byte| byte.is_ascii_digit())
    {
        return refuse();
    }
    if !id[17..]
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return refuse();
    }
    let stamp = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &id[0..4],
        &id[4..6],
        &id[6..8],
        &id[9..11],
        &id[11..13],
        &id[13..15]
    );
    let Some((year, ..)) = parse_rfc3339_utc(&stamp) else {
        return refuse();
    };
    let (max_year, ..) = civil_from_unix(now_seconds() + 24 * 60 * 60);
    if year < MIN_YEAR || year > max_year {
        return Err(invalid(format!(
            "capture_id timestamp {stamp} is not a plausible capture time; \
             check the camera's clock"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_ids_from_every_client_are_accepted_and_junk_is_not() {
        // The Pi's and the mobile app's generators.
        validate_capture_id_format("20260915T040506Z-0abcdef0").unwrap();
        validate_capture_id_format("20260830T210000Z-1234abcd").unwrap();
        // What the ESP32 used to send.
        for junk in [
            "esp32-p4-000001-12345678",
            "20260915T040506Z-0ABCDEF0",
            "20260915T040506Z-complete1",
            "19700101T000000Z-0abcdef0",
            "20260915T040506Z",
            "",
        ] {
            assert!(
                validate_capture_id_format(junk).is_err(),
                "expected {junk} to be refused"
            );
        }
    }

    #[test]
    fn absurd_capture_values_are_refused_and_text_is_trimmed() {
        let metadata = CaptureMetadata {
            firmware_version: Some("  0.4.1  ".to_owned()),
            sensor: Some("IMX519".to_owned()),
            mean_luma: Some(31),
            ..Default::default()
        }
        .validated()
        .unwrap();
        assert_eq!(metadata.firmware_version.as_deref(), Some("0.4.1"));
        assert_eq!(metadata.sensor.as_deref(), Some("imx519"));

        for absurd in [
            CaptureMetadata {
                mean_luma: Some(256),
                ..Default::default()
            },
            CaptureMetadata {
                jpeg_quality: Some(101),
                ..Default::default()
            },
            CaptureMetadata {
                exposure_us: Some(0),
                ..Default::default()
            },
            CaptureMetadata {
                width: Some(1_000_000),
                ..Default::default()
            },
            CaptureMetadata {
                analog_gain: Some(f64::NAN),
                ..Default::default()
            },
            CaptureMetadata {
                trigger: Some("telepathy".to_owned()),
                ..Default::default()
            },
            CaptureMetadata {
                sensor: Some("x".repeat(65)),
                ..Default::default()
            },
        ] {
            assert!(absurd.validated().is_err());
        }
    }

    #[test]
    fn explicit_capture_times_are_normalised_and_bad_clocks_fall_back() {
        let at = |value: &str| {
            CaptureMetadata {
                captured_at: Some(value.to_owned()),
                ..Default::default()
            }
            .validated()
            .map(|metadata| metadata.captured_at)
        };
        assert_eq!(
            at("2026-09-19T14:03:11Z").unwrap().as_deref(),
            Some("2026-09-19T14:03:11Z")
        );
        assert_eq!(
            at("2026-09-19T14:03:11.250Z").unwrap().as_deref(),
            Some("2026-09-19T14:03:11Z")
        );
        // Implausible clocks are dropped, not refused: the ID still dates it.
        assert_eq!(at("1970-01-01T00:00:00Z").unwrap(), None);
        assert_eq!(at("2999-01-01T00:00:00Z").unwrap(), None);
        // A value that is not a timestamp at all is a client bug.
        assert!(at("yesterday").is_err());
        assert!(at("2026-09-19T14:03:11+02:00").is_err());
    }

    #[test]
    fn unknown_fields_are_ignored_so_firmware_may_ship_first() {
        let parsed: CaptureMetadata = serde_json::from_str(
            r#"{"sensor":"ov5640","mean_luma":12,"future_field":{"nested":true}}"#,
        )
        .unwrap();
        assert_eq!(parsed.sensor.as_deref(), Some("ov5640"));
        assert_eq!(parsed.mean_luma, Some(12));
    }

    #[test]
    fn libcamera_sidecar_maps_onto_the_capture_object_without_overriding_it() {
        let sidecar: SensorMetadata = serde_json::from_str(
            r#"{"ExposureTime":19994,"AnalogueGain":8.0,"DigitalGain":1.02,
                "ColourTemperature":2800,"LensPosition":3.5,"AfState":2,"Lux":7.4}"#,
        )
        .unwrap_or_default();
        let merged = sidecar.merge_into(CaptureMetadata {
            exposure_us: Some(5_000),
            ..Default::default()
        });
        assert_eq!(merged.exposure_us, Some(5_000));
        assert_eq!(merged.analog_gain, Some(8.0));
        assert_eq!(merged.colour_temperature_k, Some(2800));
    }

    #[test]
    fn device_firmware_is_snapshotted_only_when_the_camera_stayed_quiet() {
        let reported = CaptureMetadata {
            firmware_version: Some("0.9.0".to_owned()),
            ..Default::default()
        }
        .with_device_firmware(Some("0.1.0"));
        assert_eq!(reported.firmware_version.as_deref(), Some("0.9.0"));
        let snapshot = CaptureMetadata::default().with_device_firmware(Some("0.1.0"));
        assert_eq!(snapshot.firmware_version.as_deref(), Some("0.1.0"));
    }
}
