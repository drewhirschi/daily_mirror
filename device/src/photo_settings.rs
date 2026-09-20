//! Persistent capture policy, shared by the physical button and admin actions.
use crate::{CameraProfile, CaptureMode};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoSettings {
    pub test_mode: bool,
    pub profile: String,
}
impl PhotoSettings {
    pub fn mode(&self) -> CaptureMode {
        if self.test_mode {
            CaptureMode::Local
        } else {
            CaptureMode::Upload
        }
    }
    pub fn validate(&self, sensor: CameraProfile) -> Result<()> {
        if !matches!(
            self.profile.as_str(),
            "configured" | "neutral" | "vibrant" | "fast"
        ) {
            bail!("Unknown photo profile");
        }
        if self.profile != "configured" && sensor != CameraProfile::Imx519 {
            bail!("These photo profiles require the IMX519 sensor");
        }
        if let Some(path) = self.tuning_file() {
            if !Path::new(path).is_file() {
                bail!("Profile tuning file is missing: {path}");
            }
        }
        Ok(())
    }
    pub fn tuning_file(&self) -> Option<&'static str> {
        match self.profile.as_str() {
            "neutral" => Some("config/imx519-af.json"),
            "vibrant" | "fast" => Some("config/imx519-portrait.json"),
            _ => None,
        }
    }
    pub fn args(&self, sensor: CameraProfile, configured: &[String]) -> Vec<String> {
        if self.profile == "configured" {
            return configured.to_vec();
        }
        let mut args = sensor.capture_args();
        if self.profile == "fast" {
            for (flag, value) in [("--width", "2328"), ("--height", "1748")] {
                let i = args.iter().position(|a| a == flag).unwrap();
                args[i + 1] = value.into();
            }
            args.extend(["--mode", "2328:1748:10:P"].map(str::to_owned));
        }
        let vibrant = self.profile != "neutral";
        args.extend(
            [
                "--saturation",
                if vibrant { "1.15" } else { "1" },
                "--sharpness",
                if vibrant { "1.15" } else { "1" },
                "--contrast",
                "1",
                "--exposure",
                if vibrant { "sport" } else { "normal" },
                "--denoise",
                "auto",
            ]
            .map(str::to_owned),
        );
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_select_real_binned_mode_and_preserve_autofocus() {
        for name in ["neutral", "vibrant", "fast"] {
            let settings = PhotoSettings {
                test_mode: true,
                profile: name.into(),
            };
            let args = settings.args(CameraProfile::Imx519, &[]);
            assert!(args.contains(&"--autofocus-mode".into()));
            let width = args.iter().position(|a| a == "--width").unwrap();
            assert_eq!(
                args[width + 1],
                if name == "fast" { "2328" } else { "4656" }
            );
            assert_eq!(args.contains(&"--mode".into()), name == "fast");
        }
        let settings = PhotoSettings {
            test_mode: false,
            profile: "configured".into(),
        };
        assert_eq!(
            settings.args(CameraProfile::Ov5647, &["custom".into()]),
            ["custom"]
        );
        assert!(
            PhotoSettings {
                test_mode: true,
                profile: "unknown".into()
            }
            .validate(CameraProfile::Imx519)
            .is_err()
        );
    }
}
