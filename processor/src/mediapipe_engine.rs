use std::{num::NonZeroU32, path::Path};

use anyhow::{Context, Result, bail};
use daily_mirror_vision_contract::{FaceResult, Landmark, NormalizedBounds, PhotoAnalysisResult};
use mediapipe::{Confidence, FaceLandmarker, Image, ModelSource, Size as MediaPipeSize};
use opencv::{
    core::{Mat, MatTraitConst, Rect, Size, Vector},
    dnn,
    imgcodecs::{IMREAD_COLOR, imdecode},
    imgproc::{COLOR_BGR2RGB, cvt_color_def},
    objdetect::{FaceDetectorYN, FaceRecognizerSF, FaceRecognizerSFTrait},
    prelude::{FaceDetectorYNTrait, FaceRecognizerSFTraitConst, MatTraitConstManual},
};
use sha2::{Digest, Sha256};

use crate::FaceProcessor;
use crate::opencv_engine::{
    OpenCvFaceEngineOptions, detector_input, normalized_bounds, normalized_embedding,
    validate_options,
};

const LANDMARK_MODEL: &str = "mediapipe-face-landmarker-float16-v1";
const LANDMARK_SCHEMA: &str = "mediapipe-face-mesh-478";
const EMBEDDING_MODEL: &str = "opencv-sface-2021dec-l2";
const MAX_FACES: u32 = 6;
const RIGHT_IRIS: usize = 468;
const LEFT_IRIS: usize = 473;
const NOSE_TIP: usize = 1;
const RIGHT_MOUTH_CORNER: usize = 61;
const LEFT_MOUTH_CORNER: usize = 291;

pub struct MediaPipeFaceEngine {
    detector: opencv::core::Ptr<FaceDetectorYN>,
    landmarker: FaceLandmarker,
    recognizer: opencv::core::Ptr<FaceRecognizerSF>,
    pipeline_version: String,
    options: OpenCvFaceEngineOptions,
}

impl MediaPipeFaceEngine {
    pub fn new(
        detector_model: &Path,
        landmarker_model: &Path,
        recognizer_model: &Path,
        pipeline_version: impl Into<String>,
    ) -> Result<Self> {
        Self::new_with_options(
            detector_model,
            landmarker_model,
            recognizer_model,
            pipeline_version,
            OpenCvFaceEngineOptions::default(),
        )
    }

    pub fn new_with_options(
        detector_model: &Path,
        landmarker_model: &Path,
        recognizer_model: &Path,
        pipeline_version: impl Into<String>,
        options: OpenCvFaceEngineOptions,
    ) -> Result<Self> {
        validate_model(detector_model, "YuNet detector")?;
        validate_model(landmarker_model, "MediaPipe face landmarker")?;
        validate_model(recognizer_model, "SFace recognizer")?;
        validate_options(options)?;
        let detector = FaceDetectorYN::create(
            &detector_model.to_string_lossy(),
            "",
            Size::new(320, 320),
            options.detector_score_threshold,
            0.3,
            5_000,
            dnn::DNN_BACKEND_OPENCV,
            dnn::DNN_TARGET_CPU,
        )
        .context("load YuNet face detector")?;
        let landmarker = FaceLandmarker::builder(ModelSource::path(landmarker_model))
            .num_faces(NonZeroU32::new(MAX_FACES).expect("maximum face count is nonzero"))
            .min_face_detection_confidence(Confidence::new(0.5)?)
            .min_face_presence_confidence(Confidence::new(0.5)?)
            .build()
            .context("load MediaPipe face landmarker")?;
        let recognizer = FaceRecognizerSF::create(
            &recognizer_model.to_string_lossy(),
            "",
            dnn::DNN_BACKEND_OPENCV,
            dnn::DNN_TARGET_CPU,
        )
        .context("load SFace recognizer")?;
        Ok(Self {
            detector,
            landmarker,
            recognizer,
            pipeline_version: pipeline_version.into(),
            options,
        })
    }

    pub fn target_label(&self) -> &'static str {
        "yunet-cpu+mediapipe-cpu+sface-cpu"
    }
}

impl FaceProcessor for MediaPipeFaceEngine {
    fn pipeline_version(&self) -> &str {
        &self.pipeline_version
    }

    fn process(&mut self, jpeg: &[u8]) -> Result<PhotoAnalysisResult> {
        let encoded = Vector::from_slice(jpeg);
        let bgr = imdecode(&encoded, IMREAD_COLOR).context("decode photo")?;
        if bgr.empty() {
            bail!("decoded photo is empty");
        }
        let size = bgr.size()?;
        if size.width <= 0 || size.height <= 0 {
            bail!("decoded photo has invalid dimensions");
        }
        let mut full_rgb = Mat::default();
        cvt_color_def(&bgr, &mut full_rgb, COLOR_BGR2RGB).context("convert photo to RGB")?;
        let full_image = Image::from_rgb(
            MediaPipeSize {
                width: size.width as u32,
                height: size.height as u32,
            },
            full_rgb.data_bytes().context("read RGB photo pixels")?,
        )
        .context("create full MediaPipe image")?;
        let full_result = self
            .landmarker
            .detect(&full_image)
            .context("run full-frame MediaPipe face landmarker")?;
        let mut candidates = Vec::with_capacity(full_result.landmarks.len());
        for face in full_result.landmarks {
            let landmarks = face
                .into_iter()
                .map(|landmark| Landmark {
                    x: landmark.point.x(),
                    y: landmark.point.y(),
                    z: landmark.point.z(),
                })
                .collect::<Vec<_>>();
            candidates.push((padded_bounds(&landmarks)?, 1.0, landmarks));
        }

        let detector_input = detector_input(&bgr, self.options.detector_max_edge)?;
        let detector_size = detector_input.size()?;
        self.detector
            .set_input_size(detector_size)
            .context("set YuNet input size")?;
        let mut detections = Mat::default();
        self.detector
            .detect(&detector_input, &mut detections)
            .context("run YuNet face detection")?;

        for row in 0..detections.rows() {
            let detection = detections.row(row)?;
            let confidence = *detection.at_2d::<f32>(0, 14)?;
            let detected_bounds = normalized_bounds(
                *detection.at_2d::<f32>(0, 0)?,
                *detection.at_2d::<f32>(0, 1)?,
                *detection.at_2d::<f32>(0, 2)?,
                *detection.at_2d::<f32>(0, 3)?,
                detector_size,
            )?;
            let crop_rect = landmarker_crop(detected_bounds, size.width, size.height)?;
            let crop_bgr = Mat::roi(&bgr, crop_rect).context("crop detected face")?;
            let mut crop_rgb = Mat::default();
            cvt_color_def(&crop_bgr, &mut crop_rgb, COLOR_BGR2RGB)
                .context("convert detected face to RGB")?;
            let media_image = Image::from_rgb(
                MediaPipeSize {
                    width: crop_rect.width as u32,
                    height: crop_rect.height as u32,
                },
                crop_rgb.data_bytes().context("read face crop RGB pixels")?,
            )
            .context("create MediaPipe face crop")?;
            let result = self
                .landmarker
                .detect(&media_image)
                .context("run MediaPipe face landmarker")?;
            let crop_faces = result
                .landmarks
                .into_iter()
                .map(|face| {
                    let landmarks = face
                        .into_iter()
                        .map(|landmark| Landmark {
                            x: (crop_rect.x as f32 + landmark.point.x() * crop_rect.width as f32)
                                / size.width as f32,
                            y: (crop_rect.y as f32 + landmark.point.y() * crop_rect.height as f32)
                                / size.height as f32,
                            z: landmark.point.z() * crop_rect.width as f32 / size.width as f32,
                        })
                        .collect::<Vec<_>>();
                    Ok((padded_bounds(&landmarks)?, landmarks))
                })
                .collect::<Result<Vec<_>>>()?;
            let Some((bounds, landmarks)) = crop_faces.into_iter().min_by(|left, right| {
                center_distance(left.0, detected_bounds)
                    .total_cmp(&center_distance(right.0, detected_bounds))
            }) else {
                continue;
            };
            if candidates
                .iter()
                .any(|candidate| intersection_over_union(candidate.0, bounds) >= 0.35)
            {
                continue;
            }
            candidates.push((bounds, confidence.clamp(0.0, 1.0), landmarks));
        }
        candidates.sort_by(|left, right| {
            face_area(right.0)
                .total_cmp(&face_area(left.0))
                .then_with(|| left.0.x.total_cmp(&right.0.x))
        });

        let mut faces = Vec::with_capacity(candidates.len());
        for (bounds, confidence, landmarks) in candidates {
            let detection = alignment_detection(&landmarks, bounds, size.width, size.height)?;
            let mut aligned = Mat::default();
            self.recognizer
                .align_crop(&bgr, &detection, &mut aligned)
                .context("align MediaPipe face for SFace")?;
            let mut feature = Mat::default();
            self.recognizer
                .feature(&aligned, &mut feature)
                .context("generate SFace embedding")?;
            faces.push(FaceResult {
                detector_confidence: confidence,
                bounds,
                landmark_model: LANDMARK_MODEL.to_owned(),
                landmark_schema: LANDMARK_SCHEMA.to_owned(),
                landmarks,
                embedding_model: EMBEDDING_MODEL.to_owned(),
                embedding: normalized_embedding(&feature)?,
            });
        }

        Ok(PhotoAnalysisResult {
            oriented_width: size.width as u32,
            oriented_height: size.height as u32,
            original_sha256: Some(format!("{:x}", Sha256::digest(jpeg))),
            processing_millis: 0,
            faces,
        })
    }
}

fn landmarker_crop(bounds: NormalizedBounds, image_width: i32, image_height: i32) -> Result<Rect> {
    let left = ((bounds.x - bounds.width * 0.55) * image_width as f32)
        .floor()
        .clamp(0.0, image_width as f32 - 1.0) as i32;
    let top = ((bounds.y - bounds.height * 0.60) * image_height as f32)
        .floor()
        .clamp(0.0, image_height as f32 - 1.0) as i32;
    let right = ((bounds.x + bounds.width * 1.55) * image_width as f32)
        .ceil()
        .clamp((left + 1) as f32, image_width as f32) as i32;
    let bottom = ((bounds.y + bounds.height * 1.40) * image_height as f32)
        .ceil()
        .clamp((top + 1) as f32, image_height as f32) as i32;
    if right <= left || bottom <= top {
        bail!("YuNet returned an invalid face crop");
    }
    Ok(Rect::new(left, top, right - left, bottom - top))
}

fn validate_model(path: &Path, label: &str) -> Result<()> {
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

fn padded_bounds(landmarks: &[Landmark]) -> Result<NormalizedBounds> {
    if landmarks.len() <= LEFT_IRIS {
        bail!("MediaPipe returned fewer than 474 face landmarks");
    }
    let mut left = 1.0f32;
    let mut top = 1.0f32;
    let mut right = 0.0f32;
    let mut bottom = 0.0f32;
    for landmark in landmarks {
        left = left.min(landmark.x);
        top = top.min(landmark.y);
        right = right.max(landmark.x);
        bottom = bottom.max(landmark.y);
    }
    let width = right - left;
    let height = bottom - top;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        bail!("MediaPipe returned invalid face landmarks");
    }
    let left = (left - width * 0.12).clamp(0.0, 1.0);
    let top = (top - height * 0.22).clamp(0.0, 1.0);
    let right = (right + width * 0.12).clamp(left, 1.0);
    let bottom = (bottom + height * 0.08).clamp(top, 1.0);
    Ok(NormalizedBounds {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn alignment_detection(
    landmarks: &[Landmark],
    bounds: NormalizedBounds,
    image_width: i32,
    image_height: i32,
) -> Result<Mat> {
    let x = |index: usize| landmarks[index].x * image_width as f32;
    let y = |index: usize| landmarks[index].y * image_height as f32;
    Mat::from_slice_2d(&[[
        bounds.x * image_width as f32,
        bounds.y * image_height as f32,
        bounds.width * image_width as f32,
        bounds.height * image_height as f32,
        x(RIGHT_IRIS),
        y(RIGHT_IRIS),
        x(LEFT_IRIS),
        y(LEFT_IRIS),
        x(NOSE_TIP),
        y(NOSE_TIP),
        x(RIGHT_MOUTH_CORNER),
        y(RIGHT_MOUTH_CORNER),
        x(LEFT_MOUTH_CORNER),
        y(LEFT_MOUTH_CORNER),
        1.0,
    ]])
    .context("build SFace alignment geometry")
}

fn face_area(bounds: NormalizedBounds) -> f32 {
    bounds.width * bounds.height
}

fn center_distance(left: NormalizedBounds, right: NormalizedBounds) -> f32 {
    let x = (left.x + left.width / 2.0) - (right.x + right.width / 2.0);
    let y = (left.y + left.height / 2.0) - (right.y + right.height / 2.0);
    x * x + y * y
}

fn intersection_over_union(left: NormalizedBounds, right: NormalizedBounds) -> f32 {
    let intersection_left = left.x.max(right.x);
    let intersection_top = left.y.max(right.y);
    let intersection_right = (left.x + left.width).min(right.x + right.width);
    let intersection_bottom = (left.y + left.height).min(right.y + right.height);
    let intersection_width = (intersection_right - intersection_left).max(0.0);
    let intersection_height = (intersection_bottom - intersection_top).max(0.0);
    let intersection = intersection_width * intersection_height;
    let union = face_area(left) + face_area(right) - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use daily_mirror_vision_contract::Landmark;

    use daily_mirror_vision_contract::NormalizedBounds;

    use super::{intersection_over_union, landmarker_crop, padded_bounds};

    #[test]
    fn dense_landmarks_produce_clipped_padded_bounds() {
        let mut landmarks = vec![
            Landmark {
                x: 0.4,
                y: 0.3,
                z: 0.0
            };
            478
        ];
        landmarks[1] = Landmark {
            x: 0.6,
            y: 0.7,
            z: 0.0,
        };
        let bounds = padded_bounds(&landmarks).unwrap();
        assert!((bounds.x - 0.376).abs() < 0.001);
        assert!((bounds.y - 0.212).abs() < 0.001);
        assert!((bounds.width - 0.248).abs() < 0.001);
        assert!((bounds.height - 0.520).abs() < 0.001);
    }

    #[test]
    fn detector_bounds_expand_into_a_landmarker_crop() {
        let crop = landmarker_crop(
            NormalizedBounds {
                x: 0.4,
                y: 0.5,
                width: 0.1,
                height: 0.16,
            },
            1_000,
            800,
        )
        .unwrap();
        assert_eq!(crop.x, 345);
        assert_eq!(crop.y, 323);
        assert_eq!(crop.width, 210);
        assert_eq!(crop.height, 257);
    }

    #[test]
    fn overlapping_full_frame_and_crop_faces_are_deduplicated() {
        let full = NormalizedBounds {
            x: 0.4,
            y: 0.3,
            width: 0.2,
            height: 0.3,
        };
        let crop = NormalizedBounds {
            x: 0.42,
            y: 0.32,
            width: 0.19,
            height: 0.29,
        };
        assert!(intersection_over_union(full, crop) > 0.7);
    }
}
