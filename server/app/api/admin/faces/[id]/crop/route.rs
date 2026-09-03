use std::io;

use axum::{
    Extension,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use image::{GenericImageView, imageops::FilterType};

use crate::{face_admin::FaceCrop, photos::PhotoStore, processing::ProcessingQueue};

const FACE_EDGE: u32 = 384;

pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(store): Extension<PhotoStore>,
    Path(id): Path<String>,
) -> Response {
    let spec = match queue.face_crop(&id).await {
        Ok(spec) => spec,
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            return StatusCode::BAD_REQUEST.into_response();
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(bytes) = (match store.original_bytes(&spec.photo_id).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match tokio::task::spawn_blocking(move || centered_face_jpeg(&bytes, &spec)).await {
        Ok(Ok(jpeg)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            jpeg,
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn centered_face_jpeg(bytes: &[u8], spec: &FaceCrop) -> io::Result<Vec<u8>> {
    let image = image::load_from_memory(bytes).map_err(io::Error::other)?;
    let (width, height) = image.dimensions();
    let width_f = width as f32;
    let height_f = height as f32;
    let bounds = spec.bounds;
    let face_width = bounds.width * width_f;
    let face_height = bounds.height * height_f;
    let center_x = face_center_x(spec) * width_f;
    let center_y = (bounds.y + bounds.height * 0.5) * height_f;
    let maximum_side = width.min(height) as f32;
    let side = (face_width.max(face_height) * 1.55)
        .clamp(32.0, maximum_side)
        .round() as u32;
    let left = (center_x - side as f32 * 0.5)
        .round()
        .clamp(0.0, width.saturating_sub(side) as f32) as u32;
    let top = (center_y - side as f32 * 0.5)
        .round()
        .clamp(0.0, height.saturating_sub(side) as f32) as u32;
    let face = image
        .crop_imm(left, top, side, side)
        .resize_exact(FACE_EDGE, FACE_EDGE, FilterType::Lanczos3);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88)
        .encode_image(&face)
        .map_err(io::Error::other)?;
    Ok(jpeg)
}

fn face_center_x(spec: &FaceCrop) -> f32 {
    let bounds_center = spec.bounds.x + spec.bounds.width * 0.5;
    let eyes = if spec.landmarks.len() >= 478 {
        spec.landmarks.get(468).zip(spec.landmarks.get(473))
    } else {
        spec.landmarks.first().zip(spec.landmarks.get(1))
    };
    eyes.map(|(right, left)| (right.x + left.x) * 0.5)
        .filter(|center| center.is_finite() && (0.0..=1.0).contains(center))
        .unwrap_or(bounds_center)
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, GenericImageView, RgbImage};

    use super::{centered_face_jpeg, face_center_x};
    use crate::face_admin::{AdminBounds, AdminLandmark, FaceCrop};

    #[test]
    fn centered_crop_is_a_square_jpeg() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(800, 600));
        let mut source = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut source)
            .encode_image(&image)
            .unwrap();
        let crop = centered_face_jpeg(
            &source,
            &FaceCrop {
                photo_id: "photo".to_owned(),
                bounds: AdminBounds {
                    x: 0.25,
                    y: 0.2,
                    width: 0.25,
                    height: 0.4,
                },
                landmarks: Vec::new(),
            },
        )
        .unwrap();
        let decoded = image::load_from_memory(&crop).unwrap();
        assert_eq!(decoded.dimensions(), (384, 384));
    }

    #[test]
    fn dense_mediapipe_crops_use_iris_centers_instead_of_first_landmarks() {
        let mut landmarks = vec![
            AdminLandmark {
                x: 0.1,
                y: 0.1,
                z: 0.0,
            };
            478
        ];
        landmarks[468].x = 0.62;
        landmarks[473].x = 0.78;
        let spec = FaceCrop {
            photo_id: "photo".to_owned(),
            bounds: AdminBounds {
                x: 0.5,
                y: 0.2,
                width: 0.4,
                height: 0.5,
            },
            landmarks,
        };
        assert!((face_center_x(&spec) - 0.7).abs() < f32::EPSILON);
    }
}
