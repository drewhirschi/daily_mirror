//! Camera: a fixture JPEG on disk. `feed.jpg` at the repository root is the
//! default. On the board this module is `esp_video` plus the IMX519 driver;
//! the runtime cannot tell the difference.

use std::path::{Path, PathBuf};

use daily_mirror_core::ports::Camera;

#[derive(Debug)]
pub struct FixtureCamera {
    path: PathBuf,
    focus_started: bool,
}

impl FixtureCamera {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            focus_started: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether `start_focus` ran before the last capture, mirroring the Pi's
    /// rule that focus tracking starts at the top of the countdown.
    pub fn focus_started(&self) -> bool {
        self.focus_started
    }
}

impl Camera for FixtureCamera {
    type Error = std::io::Error;

    fn start_focus(&mut self) -> Result<(), Self::Error> {
        self.focus_started = true;
        Ok(())
    }

    fn capture(&mut self) -> Result<Vec<u8>, Self::Error> {
        std::fs::read(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn capture_returns_the_fixture_bytes() {
        let path = std::env::temp_dir().join("daily-mirror-fixture-camera.jpg");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&[0xff, 0xd8, 0x00, 0xff, 0xd9]).unwrap();
        drop(file);

        let mut camera = FixtureCamera::new(&path);
        camera.start_focus().unwrap();
        assert!(camera.focus_started());
        assert_eq!(
            camera.capture().unwrap(),
            vec![0xff, 0xd8, 0x00, 0xff, 0xd9]
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_fixture_is_an_error_not_a_panic() {
        let mut camera = FixtureCamera::new("/nonexistent/daily-mirror/feed.jpg");
        assert!(camera.capture().is_err());
    }
}
