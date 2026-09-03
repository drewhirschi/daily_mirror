use std::{env, num::NonZeroU32, path::Path, time::Instant};

use mediapipe::{Confidence, Delegate, FaceLandmarker, Image, ModelSource};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let model_path = arguments
        .next()
        .ok_or("usage: evaluate_mediapipe <face_landmarker.task> <photo.jpg>...")?;
    let delegate = if env::var_os("MEDIAPIPE_GPU").is_some() {
        Delegate::Gpu
    } else {
        Delegate::Cpu
    };
    let confidence = env::var("MEDIAPIPE_CONFIDENCE")
        .ok()
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(0.5);
    println!("delegate: {delegate:?}, confidence: {confidence:.2}");
    let mut landmarker = FaceLandmarker::builder(ModelSource::path(model_path))
        .delegate(delegate)
        .num_faces(NonZeroU32::new(6).expect("six is nonzero"))
        .min_face_detection_confidence(Confidence::new(confidence)?)
        .min_face_presence_confidence(Confidence::new(confidence)?)
        .build()?;

    for photo_path in arguments {
        evaluate(&mut landmarker, Path::new(&photo_path))?;
    }
    Ok(())
}

fn evaluate(
    landmarker: &mut FaceLandmarker,
    photo_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let image = Image::from_file(photo_path)?;
    let started = Instant::now();
    let result = landmarker.detect(&image)?;
    println!(
        "{} ({}x{}): {} face(s) in {} ms",
        photo_path.display(),
        image.size().width,
        image.size().height,
        result.landmarks.len(),
        started.elapsed().as_millis(),
    );
    for (index, landmarks) in result.landmarks.iter().enumerate() {
        let (mut left, mut top, mut right, mut bottom) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
        for landmark in landmarks {
            left = left.min(landmark.point.x());
            top = top.min(landmark.point.y());
            right = right.max(landmark.point.x());
            bottom = bottom.max(landmark.point.y());
        }
        println!(
            "  face {}: {} landmarks bounds=({left:.3}, {top:.3}, {:.3}, {:.3})",
            index + 1,
            landmarks.len(),
            right - left,
            bottom - top,
        );
    }
    Ok(())
}
