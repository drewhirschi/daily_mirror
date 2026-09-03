use serde::{Deserialize, Serialize};

pub const DEFAULT_PIPELINE_VERSION: &str = "face-v5";
pub const MAX_CLAIM_LIMIT: u16 = 20;
pub const DEFAULT_LEASE_SECONDS: u64 = 5 * 60;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClaimRequest {
    pub worker_id: String,
    pub pipeline_version: String,
    pub limit: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClaimedPhoto {
    pub photo_id: String,
    pub lease_token: String,
    pub download_url: String,
    pub expected_bytes: Option<u64>,
    pub lease_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClaimResponse {
    pub photos: Vec<ClaimedPhoto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueueStatusRequest {
    pub pipeline_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QueueStatus {
    pub pipeline_version: String,
    pub pending: u64,
    pub leased: u64,
    pub complete: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeaseRequest {
    pub pipeline_version: String,
    pub lease_token: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompletePhotoRequest {
    pub pipeline_version: String,
    pub lease_token: String,
    pub result: PhotoAnalysisResult,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FailPhotoRequest {
    pub pipeline_version: String,
    pub lease_token: String,
    pub retryable: bool,
    pub error: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PhotoAnalysisResult {
    pub oriented_width: u32,
    pub oriented_height: u32,
    pub original_sha256: Option<String>,
    pub processing_millis: u64,
    pub faces: Vec<FaceResult>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FaceResult {
    pub detector_confidence: f32,
    pub bounds: NormalizedBounds,
    pub landmark_model: String,
    pub landmark_schema: String,
    pub landmarks: Vec<Landmark>,
    pub embedding_model: String,
    pub embedding: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct NormalizedBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Landmark {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
