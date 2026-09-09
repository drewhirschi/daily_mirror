//! Sensor-specific choices shared by normal capture, lab stills and live preview.
use crate::lab::LabSettings;
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraProfile {
    Imx519,
    Ov5647,
    Imx219,
}

impl CameraProfile {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "imx519" => Ok(Self::Imx519),
            "ov5647" => Ok(Self::Ov5647),
            "imx219" => Ok(Self::Imx219),
            _ => bail!("DAILY_MIRROR_CAMERA_PROFILE must be imx519, ov5647, or imx219"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Imx519 => "imx519",
            Self::Ov5647 => "ov5647",
            Self::Imx219 => "imx219",
        }
    }
    pub fn autofocus(self) -> bool {
        self == Self::Imx519
    }
    pub fn resolution(self) -> (u32, u32) {
        match self {
            Self::Imx519 => (4656, 3496),
            Self::Ov5647 => (2592, 1944),
            Self::Imx219 => (3280, 2464),
        }
    }
    pub fn still_args(self) -> Vec<String> {
        let (w, h) = self.resolution();
        [
            "--nopreview",
            "--timeout",
            "3000",
            "--encoding",
            "jpg",
            "--quality",
            "95",
            "--width",
            &w.to_string(),
            "--height",
            &h.to_string(),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    pub fn focus_args(self, settings: &LabSettings, capture: bool) -> Vec<String> {
        if self.autofocus() {
            settings.focus_args(capture)
        } else {
            Vec::new()
        }
    }
    pub fn preview_args(self) -> Vec<String> {
        // Preserve the calibrated IMX519 mode; libcamera selects a suitable mode for other sensors.
        if self == Self::Imx519 {
            vec!["--mode".into(), "2328:1748:10:P".into()]
        } else {
            Vec::new()
        }
    }
    pub fn capture_args(self) -> Vec<String> {
        let mut args = self.still_args();
        args.extend(self.focus_args(&LabSettings::default(), true));
        args.extend(["--metadata-format", "json", "--metadata", "-"].map(str::to_owned));
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_focus_profiles_never_emit_lens_or_autofocus_controls() {
        let manual = LabSettings {
            autofocus_mode: "manual".into(),
            ..LabSettings::default()
        };
        for p in [CameraProfile::Ov5647, CameraProfile::Imx219] {
            assert!(p.focus_args(&manual, true).is_empty());
            assert!(p.focus_args(&LabSettings::default(), false).is_empty());
            assert!(p.preview_args().is_empty());
            assert!(
                !p.capture_args()
                    .iter()
                    .any(|a| a.contains("autofocus") || a.contains("lens-position"))
            );
            let args = p.capture_args();
            let i = args.iter().position(|a| a == "--width").unwrap();
            assert_eq!(args[i + 1], p.resolution().0.to_string());
        }
    }
    #[test]
    fn imx519_keeps_autofocus_and_calibrated_preview_mode() {
        let p = CameraProfile::Imx519;
        assert!(p.capture_args().contains(&"--autofocus-window".into()));
        assert_eq!(p.preview_args(), ["--mode", "2328:1748:10:P"]);
        assert!(CameraProfile::parse("unknown").is_err());
    }
}
