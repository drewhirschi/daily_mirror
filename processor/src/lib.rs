use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use daily_mirror_vision_contract::{
    ClaimRequest, ClaimResponse, ClaimedPhoto, CompletePhotoRequest, DEFAULT_PIPELINE_VERSION,
    FailPhotoRequest, LeaseRequest, MAX_CLAIM_LIMIT, PhotoAnalysisResult, QueueStatus,
    QueueStatusRequest,
};
use reqwest::header::{AUTHORIZATION, HeaderValue};

pub mod mediapipe_engine;
pub mod opencv_engine;

impl<T: FaceProcessor + ?Sized> FaceProcessor for Box<T> {
    fn pipeline_version(&self) -> &str {
        (**self).pipeline_version()
    }

    fn process(&mut self, jpeg: &[u8]) -> Result<PhotoAnalysisResult> {
        (**self).process(jpeg)
    }
}

#[async_trait]
pub trait QueueClient: Send + Sync {
    async fn status(&self, pipeline_version: &str) -> Result<QueueStatus>;
    async fn claim(&self, request: &ClaimRequest) -> Result<Vec<ClaimedPhoto>>;
    async fn download(&self, photo: &ClaimedPhoto) -> Result<Vec<u8>>;
    async fn renew(&self, photo: &ClaimedPhoto, pipeline_version: &str) -> Result<()>;
    async fn complete(
        &self,
        photo: &ClaimedPhoto,
        pipeline_version: &str,
        result: PhotoAnalysisResult,
    ) -> Result<()>;
    async fn fail(
        &self,
        photo: &ClaimedPhoto,
        pipeline_version: &str,
        retryable: bool,
        error: &str,
    ) -> Result<()>;
}

pub trait FaceProcessor: Send {
    fn pipeline_version(&self) -> &str;
    fn process(&mut self, jpeg: &[u8]) -> Result<PhotoAnalysisResult>;
}

#[derive(Clone)]
pub struct HttpQueueClient {
    base_url: String,
    authorization: HeaderValue,
    client: reqwest::Client,
}

impl HttpQueueClient {
    pub fn new(base_url: impl Into<String>, token: &str) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            bail!("DAILY_MIRROR_SERVER_URL must be an http or https URL");
        }
        if token.len() < 16 {
            bail!("DAILY_MIRROR_PROCESSOR_TOKEN must contain at least 16 characters");
        }
        let authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .context("processor token contains invalid header characters")?;
        Ok(Self {
            base_url,
            authorization,
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn post_json<Request: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        request: &Request,
    ) -> Result<reqwest::Response> {
        self.client
            .post(self.endpoint(path))
            .header(AUTHORIZATION, self.authorization.clone())
            .json(request)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("processor API request failed: {path}"))
    }
}

#[async_trait]
impl QueueClient for HttpQueueClient {
    async fn status(&self, pipeline_version: &str) -> Result<QueueStatus> {
        self.post_json(
            "/api/processing/status",
            &QueueStatusRequest {
                pipeline_version: pipeline_version.to_owned(),
            },
        )
        .await?
        .json()
        .await
        .context("decode processing queue status")
    }

    async fn claim(&self, request: &ClaimRequest) -> Result<Vec<ClaimedPhoto>> {
        let response: ClaimResponse = self
            .post_json("/api/processing/claim", request)
            .await?
            .json()
            .await
            .context("decode claimed processing photos")?;
        Ok(response.photos)
    }

    async fn download(&self, photo: &ClaimedPhoto) -> Result<Vec<u8>> {
        let url = if photo.download_url.starts_with("http://")
            || photo.download_url.starts_with("https://")
        {
            photo.download_url.clone()
        } else {
            self.endpoint(&photo.download_url)
        };
        let bytes = self
            .client
            .get(url)
            .header(AUTHORIZATION, self.authorization.clone())
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("download photo {}", photo.photo_id))?
            .bytes()
            .await?;
        if let Some(expected) = photo.expected_bytes
            && bytes.len() as u64 != expected
        {
            bail!(
                "photo {} byte count mismatch: expected {}, received {}",
                photo.photo_id,
                expected,
                bytes.len()
            );
        }
        Ok(bytes.to_vec())
    }

    async fn renew(&self, photo: &ClaimedPhoto, pipeline_version: &str) -> Result<()> {
        self.post_json(
            &format!("/api/processing/photos/{}/renew", photo.photo_id),
            &LeaseRequest {
                pipeline_version: pipeline_version.to_owned(),
                lease_token: photo.lease_token.clone(),
            },
        )
        .await?;
        Ok(())
    }

    async fn complete(
        &self,
        photo: &ClaimedPhoto,
        pipeline_version: &str,
        result: PhotoAnalysisResult,
    ) -> Result<()> {
        self.post_json(
            &format!("/api/processing/photos/{}/complete", photo.photo_id),
            &CompletePhotoRequest {
                pipeline_version: pipeline_version.to_owned(),
                lease_token: photo.lease_token.clone(),
                result,
            },
        )
        .await?;
        Ok(())
    }

    async fn fail(
        &self,
        photo: &ClaimedPhoto,
        pipeline_version: &str,
        retryable: bool,
        error: &str,
    ) -> Result<()> {
        self.post_json(
            &format!("/api/processing/photos/{}/fail", photo.photo_id),
            &FailPhotoRequest {
                pipeline_version: pipeline_version.to_owned(),
                lease_token: photo.lease_token.clone(),
                retryable,
                error: truncate_error(error),
            },
        )
        .await?;
        Ok(())
    }
}

pub struct ProcessorRunner<Client, Engine> {
    client: Client,
    engine: Engine,
    worker_id: String,
    batch_size: u16,
    shutdown: Arc<AtomicBool>,
}

impl<Client, Engine> ProcessorRunner<Client, Engine>
where
    Client: QueueClient,
    Engine: FaceProcessor,
{
    pub fn new(
        client: Client,
        engine: Engine,
        worker_id: impl Into<String>,
        batch_size: u16,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        let worker_id = worker_id.into();
        if worker_id.is_empty() || worker_id.len() > 128 {
            bail!("worker ID must contain between 1 and 128 characters");
        }
        if batch_size == 0 || batch_size > MAX_CLAIM_LIMIT {
            bail!("batch size must be between 1 and {MAX_CLAIM_LIMIT}");
        }
        Ok(Self {
            client,
            engine,
            worker_id,
            batch_size,
            shutdown,
        })
    }

    pub async fn drain(&mut self) -> Result<DrainSummary> {
        let pipeline_version = self.engine.pipeline_version().to_owned();
        validate_pipeline_version(&pipeline_version)?;
        let mut summary = DrainSummary::default();

        while !self.shutdown.load(Ordering::SeqCst) {
            let photos = self
                .client
                .claim(&ClaimRequest {
                    worker_id: self.worker_id.clone(),
                    pipeline_version: pipeline_version.clone(),
                    limit: self.batch_size,
                })
                .await?;
            if photos.is_empty() {
                break;
            }
            summary.claimed += photos.len();
            println!("claimed {} photo(s)", photos.len());

            for (index, photo) in photos.iter().enumerate() {
                if self.shutdown.load(Ordering::SeqCst) {
                    self.release_remaining(&photos[index..], &pipeline_version)
                        .await;
                    return Ok(summary);
                }
                if let Err(error) = self.client.renew(photo, &pipeline_version).await {
                    eprintln!(
                        "skip {}: lease could not be renewed: {error:#}",
                        photo.photo_id
                    );
                    summary.skipped += 1;
                    continue;
                }

                let jpeg = match self.client.download(photo).await {
                    Ok(jpeg) => jpeg,
                    Err(error) => {
                        eprintln!("{}: download failed: {error:#}", photo.photo_id);
                        self.report_failure(photo, &pipeline_version, &error).await;
                        summary.failed += 1;
                        continue;
                    }
                };

                let started = Instant::now();
                let mut result = match self.engine.process(&jpeg) {
                    Ok(result) => result,
                    Err(error) => {
                        eprintln!("{}: processing failed: {error:#}", photo.photo_id);
                        self.report_failure(photo, &pipeline_version, &error).await;
                        summary.failed += 1;
                        continue;
                    }
                };
                result.processing_millis = started.elapsed().as_millis() as u64;

                if self.shutdown.load(Ordering::SeqCst) {
                    let _ = self
                        .client
                        .fail(photo, &pipeline_version, true, "worker interrupted")
                        .await;
                    self.release_remaining(&photos[index + 1..], &pipeline_version)
                        .await;
                    return Ok(summary);
                }

                match self.client.complete(photo, &pipeline_version, result).await {
                    Ok(()) => {
                        summary.completed += 1;
                        println!("{} complete", photo.photo_id);
                    }
                    Err(error) => {
                        summary.failed += 1;
                        eprintln!("{}: completion failed: {error:#}", photo.photo_id);
                    }
                }
            }
        }
        Ok(summary)
    }

    async fn report_failure(
        &self,
        photo: &ClaimedPhoto,
        pipeline_version: &str,
        error: &anyhow::Error,
    ) {
        if let Err(report_error) = self
            .client
            .fail(photo, pipeline_version, true, &format!("{error:#}"))
            .await
        {
            eprintln!(
                "{}: could not report processing failure: {report_error:#}",
                photo.photo_id
            );
        }
    }

    async fn release_remaining(&self, photos: &[ClaimedPhoto], pipeline_version: &str) {
        for photo in photos {
            let _ = self
                .client
                .fail(photo, pipeline_version, true, "worker interrupted")
                .await;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrainSummary {
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
}

pub fn configured_pipeline_version() -> String {
    std::env::var("DAILY_MIRROR_PROCESSING_PIPELINE")
        .unwrap_or_else(|_| DEFAULT_PIPELINE_VERSION.to_owned())
}

fn validate_pipeline_version(value: &str) -> Result<()> {
    let valid = (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(anyhow!("invalid processing pipeline version"))
    }
}

fn truncate_error(error: &str) -> String {
    let mut value = error.trim().to_owned();
    while value.len() > 500 {
        value.pop();
    }
    if value.is_empty() {
        "unknown processor error".to_owned()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use async_trait::async_trait;
    use daily_mirror_vision_contract::{
        ClaimRequest, ClaimedPhoto, PhotoAnalysisResult, QueueStatus,
    };

    use super::{FaceProcessor, ProcessorRunner, QueueClient};

    #[derive(Clone, Default)]
    struct FakeClient {
        state: Arc<Mutex<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        claims: VecDeque<Vec<ClaimedPhoto>>,
        completed: Vec<String>,
        failed: Vec<String>,
    }

    #[async_trait]
    impl QueueClient for FakeClient {
        async fn status(&self, pipeline_version: &str) -> Result<QueueStatus> {
            Ok(QueueStatus {
                pipeline_version: pipeline_version.to_owned(),
                pending: 0,
                leased: 0,
                complete: 0,
                failed: 0,
            })
        }

        async fn claim(&self, _request: &ClaimRequest) -> Result<Vec<ClaimedPhoto>> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .claims
                .pop_front()
                .unwrap_or_default())
        }

        async fn download(&self, _photo: &ClaimedPhoto) -> Result<Vec<u8>> {
            Ok(vec![0xff, 0xd8, 0xff, 0xd9])
        }

        async fn renew(&self, _photo: &ClaimedPhoto, _pipeline_version: &str) -> Result<()> {
            Ok(())
        }

        async fn complete(
            &self,
            photo: &ClaimedPhoto,
            _pipeline_version: &str,
            _result: PhotoAnalysisResult,
        ) -> Result<()> {
            self.state
                .lock()
                .unwrap()
                .completed
                .push(photo.photo_id.clone());
            Ok(())
        }

        async fn fail(
            &self,
            photo: &ClaimedPhoto,
            _pipeline_version: &str,
            _retryable: bool,
            _error: &str,
        ) -> Result<()> {
            self.state
                .lock()
                .unwrap()
                .failed
                .push(photo.photo_id.clone());
            Ok(())
        }
    }

    struct FakeEngine {
        shutdown_after: Option<usize>,
        processed: usize,
        shutdown: Arc<AtomicBool>,
    }

    impl FaceProcessor for FakeEngine {
        fn pipeline_version(&self) -> &str {
            "face-v1"
        }

        fn process(&mut self, _jpeg: &[u8]) -> Result<PhotoAnalysisResult> {
            self.processed += 1;
            if self.shutdown_after == Some(self.processed) {
                self.shutdown.store(true, Ordering::SeqCst);
            }
            Ok(PhotoAnalysisResult {
                oriented_width: 4,
                oriented_height: 3,
                original_sha256: None,
                processing_millis: 0,
                faces: Vec::new(),
            })
        }
    }

    fn photos(count: usize) -> Vec<ClaimedPhoto> {
        (0..count)
            .map(|index| ClaimedPhoto {
                photo_id: format!("photo-{index}"),
                lease_token: format!("lease-{index}"),
                download_url: format!("/photo-{index}"),
                expected_bytes: Some(4),
                lease_seconds: 300,
            })
            .collect()
    }

    #[tokio::test]
    async fn drain_completes_each_photo_independently() {
        let client = FakeClient::default();
        client.state.lock().unwrap().claims.push_back(photos(3));
        let shutdown = Arc::new(AtomicBool::new(false));
        let engine = FakeEngine {
            shutdown_after: None,
            processed: 0,
            shutdown: Arc::clone(&shutdown),
        };
        let mut runner =
            ProcessorRunner::new(client.clone(), engine, "test-worker", 20, shutdown).unwrap();

        let summary = runner.drain().await.unwrap();
        assert_eq!(summary.completed, 3);
        assert_eq!(
            client.state.lock().unwrap().completed,
            vec!["photo-0", "photo-1", "photo-2"]
        );
    }

    #[tokio::test]
    async fn interruption_does_not_complete_the_active_or_remaining_photos() {
        let client = FakeClient::default();
        client.state.lock().unwrap().claims.push_back(photos(3));
        let shutdown = Arc::new(AtomicBool::new(false));
        let engine = FakeEngine {
            shutdown_after: Some(1),
            processed: 0,
            shutdown: Arc::clone(&shutdown),
        };
        let mut runner =
            ProcessorRunner::new(client.clone(), engine, "test-worker", 20, shutdown).unwrap();

        let summary = runner.drain().await.unwrap();
        assert_eq!(summary.completed, 0);
        let state = client.state.lock().unwrap();
        assert!(state.completed.is_empty());
        assert_eq!(state.failed, vec!["photo-0", "photo-1", "photo-2"]);
    }
}
