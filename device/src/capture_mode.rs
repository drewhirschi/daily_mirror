use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    Upload,
    Local,
}
impl CaptureMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "upload" => Ok(Self::Upload),
            "local" => Ok(Self::Local),
            _ => bail!("DAILY_MIRROR_CAPTURE_MODE must be local or upload"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Local => "local",
        }
    }
    pub fn processing_message(self) -> &'static str {
        match self {
            Self::Upload => "Uploading the new photo",
            Self::Local => "Saving photo locally — uploads disabled",
        }
    }
    pub fn success_message(self) -> &'static str {
        match self {
            Self::Upload => "Photo uploaded successfully",
            Self::Local => "Photo saved locally — not queued for upload",
        }
    }
}
