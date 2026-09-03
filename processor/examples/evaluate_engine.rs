use std::{env, fs, path::Path};

use anyhow::{Context, Result};
use daily_mirror_processor::{FaceProcessor, mediapipe_engine::MediaPipeFaceEngine};

fn main() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let detector_model = arguments.next().context(
        "usage: evaluate_engine <yunet.onnx> <face_landmarker.task> <sface.onnx> <photo.jpg>...",
    )?;
    let landmarker_model = arguments
        .next()
        .context("MediaPipe model path is required")?;
    let recognizer_model = arguments.next().context("SFace model path is required")?;
    let mut engine = MediaPipeFaceEngine::new(
        Path::new(&detector_model),
        Path::new(&landmarker_model),
        Path::new(&recognizer_model),
        "face-evaluation",
    )?;

    for photo_path in arguments {
        let result = engine
            .process(&fs::read(&photo_path).with_context(|| format!("read photo {photo_path}"))?)?;
        println!(
            "{photo_path}: {} face(s), {:?}",
            result.faces.len(),
            result
                .faces
                .iter()
                .map(|face| (face.landmarks.len(), face.embedding.len(), face.bounds))
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}
