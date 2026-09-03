use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use daily_mirror_processor::mediapipe_engine::MediaPipeFaceEngine;
use daily_mirror_processor::opencv_engine::{
    InferenceTarget, OpenCvFaceEngine, OpenCvFaceEngineOptions,
};
use daily_mirror_processor::{
    FaceProcessor, HttpQueueClient, ProcessorRunner, QueueClient, configured_pipeline_version,
};

#[derive(Parser)]
#[command(about = "Intermittent Rust photo processor for Daily Mirror")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show how many photos are pending, leased, complete, or failed.
    Status,
    /// Drain queued photos with face detection, landmarks, and SFace embeddings.
    Process {
        #[arg(long, default_value_t = 20)]
        batch_size: u16,
        #[arg(long, value_enum, default_value_t = EngineKind::MediaPipe)]
        engine: EngineKind,
        #[arg(long, value_enum, default_value_t = Device::Auto)]
        device: Device,
        #[arg(
            long,
            env = "DAILY_MIRROR_MEDIAPIPE_FACE_LANDMARKER_MODEL",
            default_value = "models/face_landmarker.task"
        )]
        landmarker_model: PathBuf,
        #[arg(
            long,
            env = "DAILY_MIRROR_YUNET_MODEL",
            default_value = "models/face_detection_yunet_2023mar.onnx"
        )]
        detector_model: PathBuf,
        #[arg(
            long,
            env = "DAILY_MIRROR_SFACE_MODEL",
            default_value = "models/face_recognition_sface_2021dec.onnx"
        )]
        recognizer_model: PathBuf,
        #[arg(long, env = "DAILY_MIRROR_YUNET_MAX_EDGE", default_value_t = 1_600)]
        detector_max_edge: u32,
        #[arg(
            long,
            env = "DAILY_MIRROR_YUNET_SCORE_THRESHOLD",
            default_value_t = 0.8
        )]
        detector_score_threshold: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Device {
    #[default]
    Auto,
    Cpu,
    OpenCl,
    Cuda,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum EngineKind {
    #[default]
    MediaPipe,
    YuNet,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let client = configured_client()?;
    let pipeline = configured_pipeline_version();

    match cli.command {
        Command::Status => {
            let status = client.status(&pipeline).await?;
            println!(
                "Pipeline: {}\nPending: {}  Leased: {}  Complete: {}  Failed: {}",
                status.pipeline_version,
                status.pending,
                status.leased,
                status.complete,
                status.failed
            );
        }
        Command::Process {
            batch_size,
            engine,
            device,
            landmarker_model,
            detector_model,
            recognizer_model,
            detector_max_edge,
            detector_score_threshold,
        } => {
            let shutdown = install_shutdown_handler();
            let (engine, target_label): (Box<dyn FaceProcessor>, _) = match engine {
                EngineKind::MediaPipe => {
                    let engine = MediaPipeFaceEngine::new_with_options(
                        &detector_model,
                        &landmarker_model,
                        &recognizer_model,
                        pipeline,
                        OpenCvFaceEngineOptions {
                            detector_max_edge,
                            detector_score_threshold,
                        },
                    )?;
                    let label = engine.target_label();
                    (Box::new(engine), label)
                }
                EngineKind::YuNet => {
                    let engine = OpenCvFaceEngine::new_with_options(
                        &detector_model,
                        &recognizer_model,
                        pipeline,
                        device.into(),
                        OpenCvFaceEngineOptions {
                            detector_max_edge,
                            detector_score_threshold,
                        },
                    )?;
                    let label = engine.target_label();
                    (Box::new(engine), label)
                }
            };
            println!("inference target: {target_label}");
            let worker_id = std::env::var("DAILY_MIRROR_PROCESSOR_ID")
                .unwrap_or_else(|_| "desktop-processor".to_owned());
            let mut runner = ProcessorRunner::new(client, engine, worker_id, batch_size, shutdown)?;
            let summary = runner.drain().await?;
            println!(
                "finished: {} claimed, {} complete, {} failed, {} skipped",
                summary.claimed, summary.completed, summary.failed, summary.skipped
            );
        }
    }
    Ok(())
}

impl From<Device> for InferenceTarget {
    fn from(value: Device) -> Self {
        match value {
            Device::Auto => Self::Auto,
            Device::Cpu => Self::Cpu,
            Device::OpenCl => Self::OpenCl,
            Device::Cuda => Self::Cuda,
        }
    }
}

fn configured_client() -> Result<HttpQueueClient> {
    let server_url =
        std::env::var("DAILY_MIRROR_SERVER_URL").context("DAILY_MIRROR_SERVER_URL is required")?;
    let token = std::env::var("DAILY_MIRROR_PROCESSOR_TOKEN")
        .context("DAILY_MIRROR_PROCESSOR_TOKEN is required")?;
    HttpQueueClient::new(server_url, &token)
}

fn install_shutdown_handler() -> Arc<AtomicBool> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&shutdown);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.store(true, Ordering::SeqCst);
            eprintln!("stopping; the active photo will not be completed");
        }
        if tokio::signal::ctrl_c().await.is_ok() {
            std::process::exit(130);
        }
    });
    shutdown
}
