//! Camera: the Arducam IMX519 over MIPI CSI, through the P4's ISP and
//! hardware JPEG encoder.
//!
//! This is the last thing to bring up and the only thing that cannot be
//! rehearsed off the board: no simulator emulates a camera. Order matters —
//! everything else should already work against the fixture JPEG before the
//! sensor is on the bench.
//!
//! Constraints worth knowing before writing any of it:
//!
//! - **The ISP caps at 1920×1080.** v1 ships 1080p. Full-resolution raw
//!   bypass is not on the table.
//! - **There is no in-tree IMX519 driver.** `esp_cam_sensor` supports sixteen
//!   MIPI sensors and this is not one of them. The community port in
//!   `components.yml` carries register tables derived from the Raspberry Pi
//!   kernel driver (GPL) and offers binned modes up to 1080p.
//! - **Focus is ours to drive.** The DW9714 voice-coil motor is addressed
//!   directly over I²C, so the Pi rig's focus findings transfer: autofocus
//!   itself was never the problem, low light was. Start focus at the top of
//!   the countdown and let it track for the full three seconds while the
//!   subject walks into position.
//! - **Binned mode runs at video rate**, which is what later makes burst
//!   capture cheap: one press, N frames, one capture id, and the server picks
//!   the sharpest.

use anyhow::Result;
use daily_mirror_core::ports::Camera;

/// The ISP's ceiling, and therefore the product's resolution for v1.
pub const CAPTURE_WIDTH: u32 = 1920;
pub const CAPTURE_HEIGHT: u32 = 1080;
/// Matches the Pi's `--quality 95`.
pub const JPEG_QUALITY: u8 = 95;
/// I²C address of the DW9714 focus motor.
pub const FOCUS_MOTOR_I2C_ADDRESS: u8 = 0x0c;

pub struct Imx519Camera {
    // The `/dev/video*` file descriptor from esp_video, the V4L2 buffer ring,
    // and the I²C handle for the focus motor.
}

impl Imx519Camera {
    /// Probe the sensor, configure a 1080p binned mode, and start streaming.
    ///
    /// The first program to write against real hardware is a standalone one
    /// that does exactly this and writes a single frame to the SD card. Do
    /// that before wiring the sensor into the device loop.
    pub fn new() -> Result<Self> {
        // FFI: esp_video_init with the MIPI CSI configuration, open the video
        // device, VIDIOC_S_FMT to 1920x1080 JPEG, request and queue buffers,
        // then VIDIOC_STREAMON.
        todo!("initialize the IMX519 at {CAPTURE_WIDTH}x{CAPTURE_HEIGHT}")
    }

    /// Move the voice-coil motor. 0 is infinity, 1023 is macro.
    pub fn set_focus(&mut self, _position: u16) -> Result<()> {
        todo!("write the DW9714 at {FOCUS_MOTOR_I2C_ADDRESS:#04x}")
    }
}

impl Camera for Imx519Camera {
    type Error = anyhow::Error;

    fn start_focus(&mut self) -> Result<()> {
        // Called at the top of the three-second countdown. Kick off continuous
        // autofocus over the central portion of the frame — the Pi uses a
        // 0.2,0.15,0.6,0.7 window, which covers a standing adult at three to
        // five feet without letting the background win.
        todo!("start continuous autofocus over the portrait window")
    }

    fn capture(&mut self) -> Result<Vec<u8>> {
        // VIDIOC_DQBUF for the encoded frame, copy it into a PSRAM Vec,
        // VIDIOC_QBUF to hand the buffer back. The runtime queues the bytes to
        // the SD card before it even thinks about the network, so a capture
        // survives an upload failure.
        todo!("dequeue one JPEG frame at quality {JPEG_QUALITY}")
    }
}
