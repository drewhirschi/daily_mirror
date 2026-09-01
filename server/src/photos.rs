use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, StatusCode, header::CONTENT_LENGTH};
use rusty_s3::actions::{ListObjectsV2, S3Action as _};
use rusty_s3::{Bucket, Credentials, UrlStyle};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::ToSchema;

const PHOTO_EXTENSION: &str = "jpg";
const THUMBNAIL_EXTENSION: &str = "webp";
const THUMBNAIL_DIRECTORY: &str = ".thumbnails";
pub const THUMBNAIL_MAX_EDGE: u32 = 320;
pub const MAX_PHOTO_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_PRESIGN_SECONDS: u64 = 300;

#[derive(Clone, Debug)]
pub struct PhotoStore {
    backend: Arc<Backend>,
}

#[derive(Debug)]
enum Backend {
    Local(LocalStore),
    R2(Box<R2Store>),
}

#[derive(Debug)]
struct LocalStore {
    root: PathBuf,
}

#[derive(Debug)]
struct R2Store {
    bucket: Bucket,
    credentials: Credentials,
    prefix: String,
    client: Client,
    presign_ttl: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Photo {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug)]
pub enum PhotoRead {
    Bytes(Vec<u8>),
    Redirect(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UploadTarget {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub expires_in_seconds: Option<u64>,
}

impl PhotoStore {
    pub fn from_env() -> io::Result<Self> {
        let backend =
            std::env::var("DAILY_MIRROR_STORAGE_BACKEND").unwrap_or_else(|_| "local".to_owned());
        match backend.as_str() {
            "local" => {
                if std::env::var_os("VERCEL").is_some() {
                    return Err(invalid_config(
                        "DAILY_MIRROR_STORAGE_BACKEND=r2 is required on Vercel",
                    ));
                }
                let root = std::env::var_os("DAILY_MIRROR_STORAGE_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/photos")
                    });
                Ok(Self::new(root))
            }
            "r2" => Self::r2_from_env(),
            other => Err(invalid_config(format!(
                "DAILY_MIRROR_STORAGE_BACKEND must be local or r2, got {other:?}"
            ))),
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            backend: Arc::new(Backend::Local(LocalStore { root: root.into() })),
        }
    }

    fn r2_from_env() -> io::Result<Self> {
        let endpoint = required_env("DAILY_MIRROR_R2_ENDPOINT")?
            .parse()
            .map_err(|error| {
                invalid_config(format!("invalid DAILY_MIRROR_R2_ENDPOINT: {error}"))
            })?;
        let bucket_name = required_env("DAILY_MIRROR_R2_BUCKET")?;
        let access_key = required_env("DAILY_MIRROR_R2_ACCESS_KEY_ID")?;
        let secret_key = required_env("DAILY_MIRROR_R2_SECRET_ACCESS_KEY")?;
        let style = match std::env::var("DAILY_MIRROR_R2_URL_STYLE")
            .unwrap_or_else(|_| "virtual-host".to_owned())
            .as_str()
        {
            "virtual-host" => UrlStyle::VirtualHost,
            "path" => UrlStyle::Path,
            other => {
                return Err(invalid_config(format!(
                    "DAILY_MIRROR_R2_URL_STYLE must be virtual-host or path, got {other:?}"
                )));
            }
        };
        let bucket = Bucket::new(endpoint, style, bucket_name, "auto")
            .map_err(|error| invalid_config(format!("invalid R2 bucket configuration: {error}")))?;
        let presign_seconds = std::env::var("DAILY_MIRROR_R2_PRESIGN_SECONDS")
            .ok()
            .map(|value| {
                value.parse::<u64>().map_err(|error| {
                    invalid_config(format!("invalid DAILY_MIRROR_R2_PRESIGN_SECONDS: {error}"))
                })
            })
            .transpose()?
            .unwrap_or(DEFAULT_PRESIGN_SECONDS)
            .clamp(60, 3600);
        let prefix = normalized_prefix(
            &std::env::var("DAILY_MIRROR_R2_PREFIX").unwrap_or_else(|_| "photos".to_owned()),
        );

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(io::Error::other)?;
        Ok(Self {
            backend: Arc::new(Backend::R2(Box::new(R2Store {
                bucket,
                credentials: Credentials::new(access_key, secret_key),
                prefix,
                client,
                presign_ttl: Duration::from_secs(presign_seconds),
            }))),
        })
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.backend.as_ref(), Backend::R2(_))
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend.as_ref() {
            Backend::Local(_) => "local",
            Backend::R2(_) => "r2",
        }
    }

    pub fn storage_key(&self, id: &str) -> io::Result<String> {
        validate_capture_id(id)?;
        Ok(match self.backend.as_ref() {
            Backend::Local(_) => format!("{id}.{PHOTO_EXTENSION}"),
            Backend::R2(store) => store.key_for(id),
        })
    }

    pub async fn create_upload(
        &self,
        id: &str,
        content_type: &str,
        content_length: u64,
    ) -> io::Result<UploadTarget> {
        validate_capture_id(id)?;
        if content_type != "image/jpeg" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "content_type must be image/jpeg",
            ));
        }
        if content_length == 0 || content_length > MAX_PHOTO_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("content_length must be between 1 and {MAX_PHOTO_BYTES} bytes"),
            ));
        }

        let mut headers = BTreeMap::from([("content-type".to_owned(), content_type.to_owned())]);
        match self.backend.as_ref() {
            Backend::Local(_) => {
                headers.insert("x-capture-id".to_owned(), id.to_owned());
                Ok(UploadTarget {
                    method: "POST".to_owned(),
                    url: "/api/photos".to_owned(),
                    headers,
                    expires_in_seconds: None,
                })
            }
            Backend::R2(store) => {
                let key = store.key_for(id);
                let mut action = store
                    .bucket
                    .put_object(Some(&store.credentials), key.as_str());
                action.headers_mut().insert("content-type", content_type);
                let url = action.sign(store.presign_ttl).to_string();
                Ok(UploadTarget {
                    method: "PUT".to_owned(),
                    url,
                    headers,
                    expires_in_seconds: Some(store.presign_ttl.as_secs()),
                })
            }
        }
    }

    pub async fn save(&self, id: &str, jpeg: &[u8]) -> io::Result<Photo> {
        validate_capture_id(id)?;
        validate_jpeg(jpeg)?;
        let Backend::Local(store) = self.backend.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "raw uploads are disabled for R2; request a signed upload first",
            ));
        };

        tokio::fs::create_dir_all(&store.root).await?;
        let final_path = store.path_for(id);
        if tokio::fs::try_exists(&final_path).await? {
            return Ok(photo(id));
        }

        let temporary_path = store.root.join(format!(".{id}.{}.tmp", std::process::id()));
        let mut temporary = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await?;
        temporary.write_all(jpeg).await?;
        temporary.sync_data().await?;
        drop(temporary);

        match tokio::fs::rename(&temporary_path, &final_path).await {
            Ok(()) => Ok(photo(id)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = tokio::fs::remove_file(&temporary_path).await;
                Ok(photo(id))
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary_path).await;
                Err(error)
            }
        }
    }

    pub async fn list(&self) -> io::Result<Vec<Photo>> {
        let mut photos = match self.backend.as_ref() {
            Backend::Local(store) => store.list().await?,
            Backend::R2(store) => store.list().await?,
        };
        photos.sort_unstable_by(|left, right| right.id.cmp(&left.id));
        Ok(photos)
    }

    pub async fn read(&self, id: &str) -> io::Result<Option<PhotoRead>> {
        validate_capture_id(id)?;
        match self.backend.as_ref() {
            Backend::Local(store) => store.read(id).await,
            Backend::R2(store) => Ok(Some(PhotoRead::Redirect(store.signed_get_url(id)))),
        }
    }

    pub async fn uploaded_size(&self, id: &str) -> io::Result<Option<u64>> {
        validate_capture_id(id)?;
        match self.backend.as_ref() {
            Backend::Local(store) => match tokio::fs::metadata(store.path_for(id)).await {
                Ok(metadata) => Ok(Some(metadata.len())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
            Backend::R2(store) => store.object_size(id).await,
        }
    }

    pub async fn ensure_thumbnail(&self, id: &str) -> io::Result<bool> {
        validate_capture_id(id)?;
        if self.thumbnail_size(id).await?.is_some_and(|size| size > 0) {
            return Ok(false);
        }
        self.regenerate_thumbnail(id).await?;
        Ok(true)
    }

    pub async fn regenerate_thumbnail(&self, id: &str) -> io::Result<()> {
        let jpeg = self
            .original_bytes(id)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "photo not found"))?;
        let webp = tokio::task::spawn_blocking(move || thumbnail_webp(&jpeg))
            .await
            .map_err(io::Error::other)??;
        self.write_thumbnail(id, webp).await
    }

    pub async fn read_thumbnail(&self, id: &str) -> io::Result<Option<Vec<u8>>> {
        validate_capture_id(id)?;
        match self.backend.as_ref() {
            Backend::Local(store) => match tokio::fs::read(store.thumbnail_path_for(id)).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
            Backend::R2(store) => store.get_object(&store.thumbnail_key_for(id)).await,
        }
    }

    async fn original_bytes(&self, id: &str) -> io::Result<Option<Vec<u8>>> {
        validate_capture_id(id)?;
        match self.backend.as_ref() {
            Backend::Local(store) => match tokio::fs::read(store.path_for(id)).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            },
            Backend::R2(store) => store.get_object(&store.key_for(id)).await,
        }
    }

    async fn thumbnail_size(&self, id: &str) -> io::Result<Option<u64>> {
        match self.backend.as_ref() {
            Backend::Local(store) => {
                match tokio::fs::metadata(store.thumbnail_path_for(id)).await {
                    Ok(metadata) => Ok(Some(metadata.len())),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                }
            }
            Backend::R2(store) => {
                store
                    .object_size_for_key(&store.thumbnail_key_for(id))
                    .await
            }
        }
    }

    async fn write_thumbnail(&self, id: &str, webp: Vec<u8>) -> io::Result<()> {
        match self.backend.as_ref() {
            Backend::Local(store) => {
                let directory = store.thumbnail_directory();
                tokio::fs::create_dir_all(&directory).await?;
                let path = store.thumbnail_path_for(id);
                let temporary = directory.join(format!(".{id}.{}.tmp", std::process::id()));
                tokio::fs::write(&temporary, webp).await?;
                tokio::fs::rename(temporary, path).await
            }
            Backend::R2(store) => {
                store
                    .put_object(&store.thumbnail_key_for(id), "image/webp", webp)
                    .await
            }
        }
    }

    pub async fn delete(&self, id: &str) -> io::Result<bool> {
        validate_capture_id(id)?;
        match self.backend.as_ref() {
            Backend::Local(store) => {
                match tokio::fs::remove_file(store.thumbnail_path_for(id)).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                match tokio::fs::remove_file(store.path_for(id)).await {
                    Ok(()) => Ok(true),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                    Err(error) => Err(error),
                }
            }
            Backend::R2(store) => {
                store.delete_object(&store.thumbnail_key_for(id)).await?;
                store.delete_object(&store.key_for(id)).await?;
                Ok(true)
            }
        }
    }

    pub async fn rotate(&self, id: &str, degrees: i16) -> io::Result<()> {
        validate_capture_id(id)?;
        if !matches!(degrees, -90 | 90 | 180) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "degrees must be -90, 90, or 180",
            ));
        }
        let bytes = self
            .original_bytes(id)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "photo not found"))?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
            .map_err(io::Error::other)?;
        let rotated = match degrees {
            90 => image.rotate90(),
            -90 => image.rotate270(),
            180 => image.rotate180(),
            _ => unreachable!(),
        };
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&rotated)
            .map_err(io::Error::other)?;
        let webp = thumbnail_webp_from_image(&rotated)?;

        match self.backend.as_ref() {
            Backend::Local(store) => {
                let path = store.path_for(id);
                let temporary = path.with_extension("jpg.tmp");
                tokio::fs::write(&temporary, jpeg).await?;
                tokio::fs::rename(temporary, path).await?;
                self.write_thumbnail(id, webp).await
            }
            Backend::R2(store) => {
                store
                    .put_object(&store.key_for(id), "image/jpeg", jpeg)
                    .await?;
                self.write_thumbnail(id, webp).await
            }
        }
    }
}

impl LocalStore {
    async fn list(&self) -> io::Result<Vec<Photo>> {
        if !tokio::fs::try_exists(&self.root).await? {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&self.root).await?;
        let mut photos = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some(PHOTO_EXTENSION) {
                continue;
            }
            if let Some(id) = path.file_stem().and_then(|stem| stem.to_str())
                && validate_capture_id(id).is_ok()
            {
                photos.push(photo(id));
            }
        }
        Ok(photos)
    }

    async fn read(&self, id: &str) -> io::Result<Option<PhotoRead>> {
        match tokio::fs::read(self.path_for(id)).await {
            Ok(bytes) => Ok(Some(PhotoRead::Bytes(bytes))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.{PHOTO_EXTENSION}"))
    }

    fn thumbnail_directory(&self) -> PathBuf {
        self.root.join(THUMBNAIL_DIRECTORY)
    }

    fn thumbnail_path_for(&self, id: &str) -> PathBuf {
        self.thumbnail_directory()
            .join(format!("{id}.{THUMBNAIL_EXTENSION}"))
    }
}

impl R2Store {
    fn key_for(&self, id: &str) -> String {
        format!("{}{id}.{PHOTO_EXTENSION}", self.prefix)
    }

    fn signed_get_url(&self, id: &str) -> String {
        let key = self.key_for(id);
        self.bucket
            .get_object(Some(&self.credentials), key.as_str())
            .sign(self.presign_ttl)
            .to_string()
    }

    fn thumbnail_key_for(&self, id: &str) -> String {
        format!(
            "{}{THUMBNAIL_DIRECTORY}/{id}.{THUMBNAIL_EXTENSION}",
            self.prefix
        )
    }

    async fn get_object(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
        let action = self.bucket.get_object(Some(&self.credentials), key);
        let response = self
            .client
            .get(action.sign(self.presign_ttl))
            .send()
            .await
            .map_err(io::Error::other)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status().map_err(io::Error::other)?;
        Ok(Some(
            response.bytes().await.map_err(io::Error::other)?.to_vec(),
        ))
    }

    async fn put_object(&self, key: &str, content_type: &str, bytes: Vec<u8>) -> io::Result<()> {
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action.headers_mut().insert("content-type", content_type);
        self.client
            .put(action.sign(self.presign_ttl))
            .header("content-type", content_type)
            .body(bytes)
            .send()
            .await
            .map_err(io::Error::other)?
            .error_for_status()
            .map_err(io::Error::other)?;
        Ok(())
    }

    async fn delete_object(&self, key: &str) -> io::Result<()> {
        let action = self.bucket.delete_object(Some(&self.credentials), key);
        self.client
            .delete(action.sign(self.presign_ttl))
            .send()
            .await
            .map_err(io::Error::other)?
            .error_for_status()
            .map_err(io::Error::other)?;
        Ok(())
    }

    async fn object_size(&self, id: &str) -> io::Result<Option<u64>> {
        self.object_size_for_key(&self.key_for(id)).await
    }

    async fn object_size_for_key(&self, key: &str) -> io::Result<Option<u64>> {
        let action = self.bucket.head_object(Some(&self.credentials), key);
        let response = self
            .client
            .head(action.sign(self.presign_ttl))
            .send()
            .await
            .map_err(io::Error::other)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status().map_err(io::Error::other)?;
        let size = response
            .headers()
            .get(CONTENT_LENGTH)
            .ok_or_else(|| io::Error::other("object metadata omitted content-length"))?
            .to_str()
            .map_err(io::Error::other)?
            .parse::<u64>()
            .map_err(io::Error::other)?;
        Ok(Some(size))
    }

    async fn list(&self) -> io::Result<Vec<Photo>> {
        let mut photos = Vec::new();
        let mut continuation: Option<String> = None;
        loop {
            let mut action = ListObjectsV2::new(&self.bucket, Some(&self.credentials));
            action.with_prefix(self.prefix.as_str());
            action.with_max_keys(1000);
            if let Some(token) = continuation.as_deref() {
                action.with_continuation_token(token);
            }

            let response = self
                .client
                .get(action.sign(self.presign_ttl))
                .send()
                .await
                .map_err(io::Error::other)?
                .error_for_status()
                .map_err(io::Error::other)?;
            let body = response.text().await.map_err(io::Error::other)?;
            let page = ListObjectsV2::parse_response(&body).map_err(io::Error::other)?;

            for object in page.contents {
                if let Some(id) = self.id_from_key(&object.key) {
                    photos.push(photo(id));
                }
            }
            continuation = page.next_continuation_token;
            if continuation.is_none() {
                break;
            }
        }
        Ok(photos)
    }

    fn id_from_key<'a>(&self, key: &'a str) -> Option<&'a str> {
        let id = key
            .strip_prefix(&self.prefix)?
            .strip_suffix(&format!(".{PHOTO_EXTENSION}"))?;
        validate_capture_id(id).ok()?;
        Some(id)
    }
}

fn photo(id: &str) -> Photo {
    Photo {
        id: id.to_owned(),
        url: format!("/api/photos/{id}"),
        thumbnail_url: None,
    }
}

fn thumbnail_webp(jpeg: &[u8]) -> io::Result<Vec<u8>> {
    let image = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map_err(io::Error::other)?;
    thumbnail_webp_from_image(&image)
}

fn thumbnail_webp_from_image(image: &image::DynamicImage) -> io::Result<Vec<u8>> {
    let resized = image
        .resize(
            THUMBNAIL_MAX_EDGE,
            THUMBNAIL_MAX_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();
    let (width, height) = resized.dimensions();
    let mut webp = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
        .encode(
            resized.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(io::Error::other)?;
    Ok(webp)
}

pub fn validate_capture_id(id: &str) -> io::Result<()> {
    let valid = (8..=100).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture ID must contain 8-100 ASCII letters, numbers, dashes, or underscores",
        ))
    }
}

fn validate_jpeg(bytes: &[u8]) -> io::Result<()> {
    let looks_like_jpeg =
        bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9]);
    if looks_like_jpeg {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request body is not a complete JPEG",
        ))
    }
}

fn required_env(name: &str) -> io::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_config(format!("{name} is required when R2 storage is enabled")))
}

fn normalized_prefix(prefix: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

fn invalid_config(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use image::{GenericImageView as _, Rgb, RgbImage};
    use std::sync::Arc;
    use std::time::Duration;

    use reqwest::Client;
    use rusty_s3::{Bucket, Credentials, UrlStyle};

    use super::{
        Backend, PhotoRead, PhotoStore, R2Store, THUMBNAIL_DIRECTORY, THUMBNAIL_MAX_EDGE,
        normalized_prefix,
    };

    fn jpeg(payload: u8) -> Vec<u8> {
        vec![0xff, 0xd8, payload, 0xff, 0xd9]
    }

    fn decodable_jpeg() -> Vec<u8> {
        let image = RgbImage::from_pixel(3, 2, Rgb([24, 96, 180]));
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image)
            .unwrap();
        bytes
    }

    #[tokio::test]
    async fn saves_lists_reads_and_deduplicates_photos() {
        let root = std::env::temp_dir().join(format!(
            "daily-mirror-photo-store-test-{}",
            std::process::id()
        ));
        let _ = tokio::fs::remove_dir_all(&root).await;
        let store = PhotoStore::new(&root);

        store
            .save("20260828T204500Z-abc12345", &jpeg(1))
            .await
            .unwrap();
        store
            .save("20260828T204500Z-abc12345", &jpeg(2))
            .await
            .unwrap();
        store
            .save("20260829T071500Z-def67890", &jpeg(3))
            .await
            .unwrap();
        store
            .save("20260827T180000Z-ghi24680", &jpeg(4))
            .await
            .unwrap();

        let photos = store.list().await.unwrap();
        assert_eq!(photos.len(), 3);
        assert_eq!(photos[0].id, "20260829T071500Z-def67890");
        assert_eq!(photos[1].id, "20260828T204500Z-abc12345");
        assert_eq!(photos[2].id, "20260827T180000Z-ghi24680");
        assert_eq!(
            store
                .uploaded_size("20260828T204500Z-abc12345")
                .await
                .unwrap(),
            Some(jpeg(1).len() as u64)
        );
        assert_eq!(
            store
                .uploaded_size("20260828T204500Z-missing0")
                .await
                .unwrap(),
            None
        );
        let Some(PhotoRead::Bytes(bytes)) = store.read(&photos[1].id).await.unwrap() else {
            panic!("local photo should return bytes");
        };
        assert_eq!(bytes, jpeg(1));

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn local_upload_target_preserves_the_direct_upload_path() {
        let store = PhotoStore::new(std::env::temp_dir());
        let target = store
            .create_upload("20260828T204500Z-abc12345", "image/jpeg", 1024)
            .await
            .unwrap();
        assert_eq!(target.method, "POST");
        assert_eq!(target.url, "/api/photos");
        assert_eq!(
            target.headers.get("x-capture-id").map(String::as_str),
            Some("20260828T204500Z-abc12345")
        );
    }

    #[tokio::test]
    async fn rotates_and_deletes_local_photos() {
        let root = std::env::temp_dir().join(format!(
            "daily-mirror-photo-edit-test-{}",
            std::process::id()
        ));
        let _ = tokio::fs::remove_dir_all(&root).await;
        let store = PhotoStore::new(&root);
        let id = "20260829T071500Z-edit1234";
        store.save(id, &decodable_jpeg()).await.unwrap();

        assert!(store.ensure_thumbnail(id).await.unwrap());
        assert!(!store.ensure_thumbnail(id).await.unwrap());
        let thumbnail = store.read_thumbnail(id).await.unwrap().unwrap();
        let thumbnail =
            image::load_from_memory_with_format(&thumbnail, image::ImageFormat::WebP).unwrap();
        assert_eq!(thumbnail.width(), THUMBNAIL_MAX_EDGE);
        assert!(thumbnail.height() <= THUMBNAIL_MAX_EDGE);

        tokio::fs::write(
            root.join(THUMBNAIL_DIRECTORY).join(format!("{id}.webp")),
            [],
        )
        .await
        .unwrap();
        assert!(store.ensure_thumbnail(id).await.unwrap());
        assert!(!store.read_thumbnail(id).await.unwrap().unwrap().is_empty());

        store.rotate(id, 90).await.unwrap();
        let Some(PhotoRead::Bytes(bytes)) = store.read(id).await.unwrap() else {
            panic!("rotated photo should remain readable");
        };
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (2, 3)
        );
        let thumbnail = store.read_thumbnail(id).await.unwrap().unwrap();
        let thumbnail =
            image::load_from_memory_with_format(&thumbnail, image::ImageFormat::WebP).unwrap();
        assert_eq!(thumbnail.height(), THUMBNAIL_MAX_EDGE);
        assert!(thumbnail.width() <= THUMBNAIL_MAX_EDGE);
        assert!(store.delete(id).await.unwrap());
        assert!(store.read(id).await.unwrap().is_none());
        assert!(store.read_thumbnail(id).await.unwrap().is_none());
        assert!(!store.delete(id).await.unwrap());

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn r2_upload_target_is_object_scoped_and_short_lived() {
        let bucket = Bucket::new(
            "https://example.r2.cloudflarestorage.com".parse().unwrap(),
            UrlStyle::VirtualHost,
            "daily-mirror",
            "auto",
        )
        .unwrap();
        let store = PhotoStore {
            backend: Arc::new(Backend::R2(Box::new(R2Store {
                bucket,
                credentials: Credentials::new("access-key", "secret-key"),
                prefix: "photos/".to_owned(),
                client: Client::new(),
                presign_ttl: Duration::from_secs(300),
            }))),
        };

        let target = store
            .create_upload("20260828T204500Z-abc12345", "image/jpeg", 1024)
            .await
            .unwrap();
        assert_eq!(target.method, "PUT");
        assert_eq!(target.expires_in_seconds, Some(300));
        assert!(target.url.starts_with(
            "https://daily-mirror.example.r2.cloudflarestorage.com/photos/20260828T204500Z-abc12345.jpg?"
        ));
        assert!(target.url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(target.url.contains("X-Amz-Expires=300"));
        assert!(
            target
                .url
                .contains("X-Amz-SignedHeaders=content-type%3Bhost")
        );
        assert_eq!(
            target.headers.get("content-type").map(String::as_str),
            Some("image/jpeg")
        );
    }

    #[tokio::test]
    async fn rejects_invalid_ids_non_jpegs_and_oversized_grants() {
        let store = PhotoStore::new(std::env::temp_dir());
        assert!(store.save("../escape", &jpeg(1)).await.is_err());
        assert!(
            store
                .save("20260828T204500Z-abc12345", b"not a jpeg")
                .await
                .is_err()
        );
        assert!(
            store
                .create_upload("20260828T204500Z-abc12345", "image/jpeg", u64::MAX)
                .await
                .is_err()
        );
    }

    #[test]
    fn normalizes_r2_prefixes() {
        assert_eq!(normalized_prefix("photos"), "photos/");
        assert_eq!(normalized_prefix("/daily/photos/"), "daily/photos/");
        assert_eq!(normalized_prefix("/"), "");
    }
}
