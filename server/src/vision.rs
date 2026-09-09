//! One durable, leased photo per hosted invocation. No inference in web bundles.
use crate::{
    background::{self, RunRequest},
    photos::PhotoStore,
    processing::{ProcessingQueue, active_pipeline_version},
};
use axum::http::StatusCode;
use daily_mirror_processor::{FaceProcessor, mediapipe_engine::MediaPipeFaceEngine};
use daily_mirror_vision_contract::PhotoAnalysisResult;
use nextrs::WaitUntil;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

static ENGINE: Mutex<Option<MediaPipeFaceEngine>> = Mutex::new(None);

#[derive(Serialize)]
pub struct RunReport {
    pub outcome: &'static str,
    pub photo_id: Option<String>,
    pub faces: usize,
    pub elapsed_millis: u64,
    pub model_load_millis: u64,
    pub peak_rss_kib: Option<u64>,
}

pub async fn run(
    queue: ProcessingQueue,
    store: PhotoStore,
    wait: WaitUntil,
    request: RunRequest,
) -> Result<RunReport, StatusCode> {
    let started = Instant::now();
    let pipeline = active_pipeline_version().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let photo = queue
        .claim_hosted(&pipeline, request.photo_id.as_deref())
        .await
        .map_err(|e| e.status_code())?;
    let Some(photo) = photo else {
        return Ok(RunReport {
            outcome: "idle-or-busy",
            photo_id: None,
            faces: 0,
            elapsed_millis: started.elapsed().as_millis() as u64,
            model_load_millis: 0,
            peak_rss_kib: peak_rss(),
        });
    };
    let result = async {
        let jpeg = store.original_bytes(&photo.photo_id).await
            .map_err(|_| "original download failed".to_owned())?
            .ok_or_else(|| "original is missing".to_owned())?;
        if photo.expected_bytes.is_some_and(|n| n != jpeg.len() as u64) {
            return Err("original byte count changed".to_owned());
        }
        let version = pipeline.clone();
        let mut task = tokio::task::spawn_blocking(move || analyze(&jpeg, &version));
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        let mut lost = false;
        let analyzed = loop {
            tokio::select! {
                result = &mut task => break result.map_err(|_| "inference task panicked".to_owned())?,
                _ = heartbeat.tick() => {
                    if queue.renew(&photo.photo_id, &pipeline, &photo.lease_token).await.is_err() { lost = true; }
                }
            }
        }?;
        if lost { return Err("lease lost during inference".to_owned()); }
        let (result, model_load_millis) = analyzed;
        let faces = result.faces.len();
        queue.complete(&photo.photo_id, &pipeline, &photo.lease_token, &result).await
            .map_err(|_| "result commit failed or lease was invalidated".to_owned())?;
        Ok((faces, model_load_millis))
    }.await;
    let report = match result {
        Ok((faces, model_load_millis)) => RunReport {
            outcome: "complete",
            photo_id: Some(photo.photo_id.clone()),
            faces,
            elapsed_millis: started.elapsed().as_millis() as u64,
            model_load_millis,
            peak_rss_kib: peak_rss(),
        },
        Err(error) => {
            let _ = queue
                .fail(&photo.photo_id, &pipeline, &photo.lease_token, true, &error)
                .await;
            eprintln!("hosted photo {} failed: {}", photo.photo_id, error);
            if request.continue_queue {
                background::notify(&wait, None);
            }
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    println!(
        "hosted_processing {}",
        serde_json::to_string(&report).unwrap_or_default()
    );
    if request.continue_queue {
        background::notify(&wait, None);
    }
    Ok(report)
}

fn analyze(jpeg: &[u8], pipeline: &str) -> Result<(PhotoAnalysisResult, u64), String> {
    let mut cached = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    let mut load_millis = 0;
    if cached.is_none() {
        let start = Instant::now();
        let library = std::env::var("MEDIAPIPE_LIB")
            .map_err(|_| "MEDIAPIPE_LIB must point to the packaged library")?;
        if !Path::new(&library).is_file() {
            return Err("packaged MediaPipe library is missing".to_owned());
        }
        let root = Path::new("resources/vision");
        for (file, checksum) in [
            (
                "face_detection_yunet_2023mar.onnx",
                "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
            ),
            (
                "face_landmarker.task",
                "64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff",
            ),
            (
                "face_recognition_sface_2021dec.onnx",
                "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
            ),
        ] {
            let bytes = std::fs::read(root.join(file))
                .map_err(|_| format!("packaged model missing: {file}"))?;
            if format!("{:x}", Sha256::digest(bytes)) != checksum {
                return Err(format!("model checksum mismatch: {file}"));
            }
        }
        *cached = Some(
            MediaPipeFaceEngine::new(
                &root.join("face_detection_yunet_2023mar.onnx"),
                &root.join("face_landmarker.task"),
                &root.join("face_recognition_sface_2021dec.onnx"),
                pipeline,
            )
            .map_err(|e| format!("model initialization: {e}"))?,
        );
        load_millis = start.elapsed().as_millis() as u64;
    }
    let start = Instant::now();
    let mut result = cached
        .as_mut()
        .unwrap()
        .process(jpeg)
        .map_err(|e| format!("inference: {e}"))?;
    result.processing_millis = start.elapsed().as_millis() as u64;
    Ok((result, load_millis))
}

fn peak_rss() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
