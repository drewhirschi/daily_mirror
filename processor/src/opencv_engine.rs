use std::path::Path;

use anyhow::{Context, Result, bail};
use daily_mirror_vision_contract::{FaceResult, Landmark, NormalizedBounds, PhotoAnalysisResult};
use opencv::core::{Mat, MatTraitConst, Size, Vector};
use opencv::dnn::{self, Backend, Target};
use opencv::imgcodecs::{IMREAD_COLOR, imdecode};
use opencv::imgproc::{INTER_AREA, resize};
use opencv::objdetect::{FaceDetectorYN, FaceRecognizerSF};
use opencv::prelude::{
    FaceDetectorYNTrait, FaceRecognizerSFTrait, FaceRecognizerSFTraitConst, MatTraitConstManual,
};
use sha2::{Digest, Sha256};

use crate::FaceProcessor;

const LANDMARK_MODEL: &str = "opencv-yunet-2023mar";
const LANDMARK_SCHEMA: &str = "yunet-5-right-eye-left-eye-nose-right-mouth-left-mouth";
const EMBEDDING_MODEL: &str = "opencv-sface-2021dec-l2";
pub(crate) const DEFAULT_DETECTOR_MAX_EDGE: u32 = 1_600;
pub(crate) const DEFAULT_DETECTOR_SCORE_THRESHOLD: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenCvFaceEngineOptions {
    pub detector_max_edge: u32,
    pub detector_score_threshold: f32,
}

impl Default for OpenCvFaceEngineOptions {
    fn default() -> Self {
        Self {
            detector_max_edge: DEFAULT_DETECTOR_MAX_EDGE,
            detector_score_threshold: DEFAULT_DETECTOR_SCORE_THRESHOLD,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InferenceTarget {
    #[default]
    Auto,
    Cpu,
    OpenCl,
    Cuda,
}

impl InferenceTarget {
    fn resolve(self) -> Self {
        match self {
            Self::Auto if target_available(Backend::DNN_BACKEND_CUDA, Target::DNN_TARGET_CUDA) => {
                Self::Cuda
            }
            Self::Auto
                if target_available(Backend::DNN_BACKEND_OPENCV, Target::DNN_TARGET_OPENCL) =>
            {
                Self::OpenCl
            }
            Self::Auto => Self::Cpu,
            selected => selected,
        }
    }

    fn backend_and_target(self) -> (i32, i32) {
        match self {
            Self::Auto | Self::Cpu => (dnn::DNN_BACKEND_OPENCV, dnn::DNN_TARGET_CPU),
            Self::OpenCl => (dnn::DNN_BACKEND_OPENCV, dnn::DNN_TARGET_OPENCL),
            Self::Cuda => (dnn::DNN_BACKEND_CUDA, dnn::DNN_TARGET_CUDA),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::OpenCl => "opencl",
            Self::Cuda => "cuda",
        }
    }
}

pub struct OpenCvFaceEngine {
    detector: opencv::core::Ptr<FaceDetectorYN>,
    recognizer: opencv::core::Ptr<FaceRecognizerSF>,
    pipeline_version: String,
    target: InferenceTarget,
    options: OpenCvFaceEngineOptions,
}

impl OpenCvFaceEngine {
    pub fn new(
        detector_model: &Path,
        recognizer_model: &Path,
        pipeline_version: impl Into<String>,
        requested_target: InferenceTarget,
    ) -> Result<Self> {
        Self::new_with_options(
            detector_model,
            recognizer_model,
            pipeline_version,
            requested_target,
            OpenCvFaceEngineOptions::default(),
        )
    }

    pub fn new_with_options(
        detector_model: &Path,
        recognizer_model: &Path,
        pipeline_version: impl Into<String>,
        requested_target: InferenceTarget,
        options: OpenCvFaceEngineOptions,
    ) -> Result<Self> {
        validate_model(detector_model, "YuNet detector")?;
        validate_model(recognizer_model, "SFace recognizer")?;
        validate_options(options)?;
        let target = requested_target.resolve();
        if requested_target != InferenceTarget::Auto && !target.is_available() {
            bail!(
                "requested OpenCV DNN target '{}' is unavailable in this OpenCV build",
                requested_target.label()
            );
        }
        let (backend_id, target_id) = target.backend_and_target();
        let detector_path = detector_model.to_string_lossy();
        let recognizer_path = recognizer_model.to_string_lossy();
        let detector = FaceDetectorYN::create(
            &detector_path,
            "",
            Size::new(320, 320),
            options.detector_score_threshold,
            0.3,
            5_000,
            backend_id,
            target_id,
        )
        .context("load YuNet face detector")?;
        let recognizer = FaceRecognizerSF::create(&recognizer_path, "", backend_id, target_id)
            .context("load SFace recognizer")?;
        Ok(Self {
            detector,
            recognizer,
            pipeline_version: pipeline_version.into(),
            target,
            options,
        })
    }

    pub fn target_label(&self) -> &'static str {
        self.target.label()
    }
}

impl FaceProcessor for OpenCvFaceEngine {
    fn pipeline_version(&self) -> &str {
        &self.pipeline_version
    }

    fn process(&mut self, jpeg: &[u8]) -> Result<PhotoAnalysisResult> {
        let encoded = Vector::from_slice(jpeg);
        let image = imdecode(&encoded, IMREAD_COLOR).context("decode photo")?;
        if image.empty() {
            bail!("decoded photo is empty");
        }
        let original_size = image.size()?;
        if original_size.width <= 0 || original_size.height <= 0 {
            bail!("decoded photo has invalid dimensions");
        }
        let detector_input = detector_input(&image, self.options.detector_max_edge)?;
        let detector_size = detector_input.size()?;

        self.detector
            .set_input_size(detector_size)
            .context("set YuNet input size")?;
        let mut detections = Mat::default();
        self.detector
            .detect(&detector_input, &mut detections)
            .context("run YuNet face detection")?;

        let mut faces = Vec::with_capacity(detections.rows().max(0) as usize);
        for row in 0..detections.rows() {
            let detection = detections.row(row)?;
            let confidence = *detection.at_2d::<f32>(0, 14)?;
            let x = *detection.at_2d::<f32>(0, 0)?;
            let y = *detection.at_2d::<f32>(0, 1)?;
            let width = *detection.at_2d::<f32>(0, 2)?;
            let height = *detection.at_2d::<f32>(0, 3)?;
            let bounds = normalized_bounds(x, y, width, height, detector_size)?;

            let landmarks = (0..5)
                .map(|index| {
                    let landmark_x = *detection.at_2d::<f32>(0, 4 + index * 2)?;
                    let landmark_y = *detection.at_2d::<f32>(0, 5 + index * 2)?;
                    Ok(Landmark {
                        x: landmark_x / detector_size.width as f32,
                        y: landmark_y / detector_size.height as f32,
                        z: 0.0,
                    })
                })
                .collect::<opencv::Result<Vec<_>>>()?;

            let mut aligned = Mat::default();
            self.recognizer
                .align_crop(&detector_input, &detection, &mut aligned)
                .context("align detected face")?;
            let mut feature = Mat::default();
            self.recognizer
                .feature(&aligned, &mut feature)
                .context("generate SFace embedding")?;
            let embedding = normalized_embedding(&feature)?;

            faces.push(FaceResult {
                detector_confidence: confidence.clamp(0.0, 1.0),
                bounds,
                landmark_model: LANDMARK_MODEL.to_owned(),
                landmark_schema: LANDMARK_SCHEMA.to_owned(),
                landmarks,
                embedding_model: EMBEDDING_MODEL.to_owned(),
                embedding,
            });
        }

        Ok(PhotoAnalysisResult {
            oriented_width: original_size.width as u32,
            oriented_height: original_size.height as u32,
            original_sha256: Some(format!("{:x}", Sha256::digest(jpeg))),
            processing_millis: 0,
            faces,
        })
    }
}

impl InferenceTarget {
    fn is_available(self) -> bool {
        let (backend, target) = match self {
            Self::Auto | Self::Cpu => return true,
            Self::OpenCl => (Backend::DNN_BACKEND_OPENCV, Target::DNN_TARGET_OPENCL),
            Self::Cuda => (Backend::DNN_BACKEND_CUDA, Target::DNN_TARGET_CUDA),
        };
        target_available(backend, target)
    }
}

fn target_available(backend: Backend, target: Target) -> bool {
    dnn::get_available_targets(backend)
        .map(|targets| targets.iter().any(|available| available == target))
        .unwrap_or(false)
}

pub(crate) fn validate_model(path: &Path, label: &str) -> Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("{label} model is missing at {}", path.display()))?;
    if !metadata.is_file() || metadata.len() < 1_024 {
        bail!(
            "{label} model at {} is not a valid model file",
            path.display()
        );
    }
    Ok(())
}

pub(crate) fn validate_options(options: OpenCvFaceEngineOptions) -> Result<()> {
    if !(320..=4_096).contains(&options.detector_max_edge) {
        bail!("YuNet detector max edge must be between 320 and 4096 pixels");
    }
    if !(0.0..=1.0).contains(&options.detector_score_threshold)
        || !options.detector_score_threshold.is_finite()
    {
        bail!("YuNet detector score threshold must be between 0 and 1");
    }
    Ok(())
}

pub(crate) fn detector_input(image: &Mat, max_edge: u32) -> Result<Mat> {
    let size = image.size()?;
    let largest = size.width.max(size.height);
    if largest <= max_edge as i32 {
        return Ok(image.clone());
    }
    let scale = f64::from(max_edge) / f64::from(largest);
    let target = Size::new(
        (f64::from(size.width) * scale).round() as i32,
        (f64::from(size.height) * scale).round() as i32,
    );
    let mut resized = Mat::default();
    resize(image, &mut resized, target, 0.0, 0.0, INTER_AREA).context("resize photo for YuNet")?;
    Ok(resized)
}

pub(crate) fn normalized_bounds(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    size: Size,
) -> Result<NormalizedBounds> {
    let image_width = size.width as f32;
    let image_height = size.height as f32;
    let left = x.clamp(0.0, image_width);
    let top = y.clamp(0.0, image_height);
    let right = (x + width).clamp(left, image_width);
    let bottom = (y + height).clamp(top, image_height);
    if right <= left || bottom <= top {
        bail!("YuNet returned an empty face bounding box");
    }
    Ok(NormalizedBounds {
        x: left / image_width,
        y: top / image_height,
        width: (right - left) / image_width,
        height: (bottom - top) / image_height,
    })
}

pub(crate) fn normalized_embedding(feature: &Mat) -> Result<Vec<f32>> {
    let values = feature
        .data_typed::<f32>()
        .context("read SFace embedding")?;
    if values.is_empty() {
        bail!("SFace returned an empty embedding");
    }
    let norm = values
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        bail!("SFace returned an invalid embedding");
    }
    Ok(values.iter().map(|value| *value / norm as f32).collect())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use opencv::core::Size;

    use crate::FaceProcessor;

    use super::{
        InferenceTarget, OpenCvFaceEngine, OpenCvFaceEngineOptions, normalized_bounds,
        validate_options,
    };

    #[test]
    fn bounds_are_clipped_and_normalized() {
        let bounds = normalized_bounds(-5.0, 10.0, 45.0, 50.0, Size::new(100, 200)).unwrap();
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.y, 0.05);
        assert_eq!(bounds.width, 0.4);
        assert_eq!(bounds.height, 0.25);
    }

    #[test]
    fn detector_options_reject_invalid_values() {
        assert!(
            validate_options(OpenCvFaceEngineOptions {
                detector_max_edge: 319,
                ..OpenCvFaceEngineOptions::default()
            })
            .is_err()
        );
        assert!(
            validate_options(OpenCvFaceEngineOptions {
                detector_score_threshold: f32::NAN,
                ..OpenCvFaceEngineOptions::default()
            })
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires downloaded OpenCV Zoo models"]
    fn official_models_process_a_real_photo() {
        let mut engine = OpenCvFaceEngine::new(
            Path::new("models/face_detection_yunet_2023mar.onnx"),
            Path::new("models/face_recognition_sface_2021dec.onnx"),
            "face-v1",
            InferenceTarget::Cpu,
        )
        .unwrap();
        let jpeg = fs::read("../feed.jpg").unwrap();
        let result = engine.process(&jpeg).unwrap();
        assert!(result.oriented_width > 0);
        assert!(result.oriented_height > 0);
        assert!(result.faces.iter().all(|face| face.embedding.len() == 128));
        println!("detected {} face(s)", result.faces.len());
    }
}
