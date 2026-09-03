use std::{env, fs, path::Path};

use anyhow::{Context, Result};
use opencv::{
    core::{Mat, MatTraitConst, Size, Vector},
    imgcodecs::{IMREAD_COLOR, imdecode},
    imgproc::{INTER_AREA, resize},
    objdetect::{FaceDetectorYN, FaceDetectorYNTrait},
};

const CONFIGURATIONS: &[(i32, f32)] = &[
    (0, 0.9),
    (0, 0.6),
    (1_600, 0.8),
    (1_600, 0.6),
    (1_200, 0.6),
    (960, 0.5),
];

fn main() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let model_path = arguments
        .next()
        .context("usage: evaluate_yunet <model.onnx> <photo.jpg>...")?;
    for photo_path in arguments {
        evaluate(Path::new(&model_path), Path::new(&photo_path))?;
    }
    Ok(())
}

fn evaluate(model_path: &Path, photo_path: &Path) -> Result<()> {
    let bytes =
        fs::read(photo_path).with_context(|| format!("read photo {}", photo_path.display()))?;
    let image = imdecode(&Vector::from_slice(&bytes), IMREAD_COLOR).context("decode photo")?;
    let original_size = image.size()?;
    println!(
        "{} ({}x{})",
        photo_path.display(),
        original_size.width,
        original_size.height
    );

    for &(max_edge, score_threshold) in CONFIGURATIONS {
        let (input, scale) = resized_input(&image, max_edge)?;
        let input_size = input.size()?;
        let mut detector = FaceDetectorYN::create(
            &model_path.to_string_lossy(),
            "",
            input_size,
            score_threshold,
            0.3,
            5_000,
            opencv::dnn::DNN_BACKEND_OPENCV,
            opencv::dnn::DNN_TARGET_CPU,
        )?;
        let mut detections = Mat::default();
        detector.detect(&input, &mut detections)?;
        let label = if max_edge == 0 {
            "original".to_owned()
        } else {
            format!("max-edge-{max_edge}")
        };
        println!(
            "  {label} threshold={score_threshold:.1}: {} face(s)",
            detections.rows()
        );
        for row in 0..detections.rows() {
            let detection = detections.row(row)?;
            let x = *detection.at_2d::<f32>(0, 0)? / scale;
            let y = *detection.at_2d::<f32>(0, 1)? / scale;
            let width = *detection.at_2d::<f32>(0, 2)? / scale;
            let height = *detection.at_2d::<f32>(0, 3)? / scale;
            let confidence = *detection.at_2d::<f32>(0, 14)?;
            println!(
                "    {confidence:.3} bounds=({:.3}, {:.3}, {:.3}, {:.3})",
                x / original_size.width as f32,
                y / original_size.height as f32,
                width / original_size.width as f32,
                height / original_size.height as f32,
            );
        }
    }
    Ok(())
}

fn resized_input(image: &Mat, max_edge: i32) -> Result<(Mat, f32)> {
    let size = image.size()?;
    let largest = size.width.max(size.height);
    if max_edge == 0 || largest <= max_edge {
        return Ok((image.clone(), 1.0));
    }

    let scale = max_edge as f32 / largest as f32;
    let target = Size::new(
        (size.width as f32 * scale).round() as i32,
        (size.height as f32 * scale).round() as i32,
    );
    let mut resized = Mat::default();
    resize(image, &mut resized, target, 0.0, 0.0, INTER_AREA)?;
    Ok((resized, scale))
}
