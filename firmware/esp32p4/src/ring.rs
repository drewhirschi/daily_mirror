//! Ring: a WS2812 NeoPixel ring on the RMT peripheral.
//!
//! The core hands down a `RingPattern` — a meaning, not a colour — and this
//! module owns the animation. `set` only records the pattern; a timer task
//! calls `render` at about 50 Hz so breathing, chasing and pulsing are
//! genuinely animated instead of stepping once per button poll.
//!
//! Bring-up step one, with the button: the ring is the only way the device
//! says anything before Wi-Fi exists.

use daily_mirror_core::ports::Ring;
use daily_mirror_core::ring::RingPattern;

/// GPIO driving the ring's data line.
pub const RING_PIN: u32 = 5;
/// Pixels in the ring. The chase and the amber fill divide by this.
pub const RING_PIXELS: usize = 16;
/// Animation rate. Fast enough that a breathe looks smooth.
pub const RING_FRAME_MS: u64 = 20;

/// sRGB, before the gamma curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const OFF: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const DIM_WHITE: Self = Self::new(40, 40, 40);
    /// The plan's amber. Warmer than yellow so it reads as "attention".
    pub const AMBER: Self = Self::new(255, 130, 0);
    pub const BLUE: Self = Self::new(0, 90, 255);
    pub const GREEN: Self = Self::new(0, 220, 60);
    pub const RED: Self = Self::new(255, 0, 0);

    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Scale toward black. Used by every pulse and breathe.
    pub fn scale(self, numerator: u16) -> Self {
        let apply = |value: u8| ((value as u16 * numerator) / 255) as u8;
        Self::new(apply(self.red), apply(self.green), apply(self.blue))
    }
}

pub struct Ws2812Ring {
    pattern: RingPattern,
    /// Frames since the pattern last changed; drives every animation.
    frame: u64,
}

impl Ws2812Ring {
    /// Claim the RMT channel and blank the ring.
    pub fn new() -> anyhow::Result<Self> {
        // FFI: rmt_new_tx_channel + rmt_new_bytes_encoder with the WS2812
        // 0.4/0.85 µs bit timings at a 10 MHz resolution.
        todo!("create the RMT TX channel on GPIO {RING_PIN} for {RING_PIXELS} pixels")
    }

    /// Compute this frame's pixels. Pure, so the pattern vocabulary can be
    /// unit-tested on a laptop once the FFI above exists.
    pub fn frame_pixels(&self) -> [Rgb; RING_PIXELS] {
        let frame = self.frame;
        let mut pixels = [Rgb::OFF; RING_PIXELS];
        match self.pattern {
            RingPattern::Off => {}
            RingPattern::SolidWhite => pixels.fill(Rgb::WHITE),
            RingPattern::SolidRed => pixels.fill(Rgb::RED),
            RingPattern::DimWhiteBreathe => pixels.fill(Rgb::DIM_WHITE.scale(triangle(frame, 150))),
            RingPattern::AmberFill { fraction_percent } => {
                let lit = (RING_PIXELS * fraction_percent as usize).div_ceil(100);
                for pixel in pixels.iter_mut().take(lit) {
                    *pixel = Rgb::AMBER;
                }
            }
            RingPattern::AmberChase => {
                // A three-pixel comet, one step every four frames.
                let head = (frame / 4) as usize % RING_PIXELS;
                for offset in 0..3 {
                    let index = (head + RING_PIXELS - offset) % RING_PIXELS;
                    pixels[index] = Rgb::AMBER.scale(255 - (offset as u16 * 80));
                }
            }
            RingPattern::AmberFastPulse => pixels.fill(Rgb::AMBER.scale(triangle(frame, 15))),
            RingPattern::AmberSlowPulse => pixels.fill(Rgb::AMBER.scale(triangle(frame, 50))),
            RingPattern::AmberRapidPulse => pixels.fill(Rgb::AMBER.scale(triangle(frame, 6))),
            RingPattern::BlueSlowPulse => pixels.fill(Rgb::BLUE.scale(triangle(frame, 50))),
            RingPattern::GreenFlash => pixels.fill(Rgb::GREEN),
            RingPattern::RedFlash => pixels.fill(Rgb::RED),
            RingPattern::RedTriplePulse => {
                // Three pulses then dark; the state machine sets the next
                // pattern immediately after, so this only has to look right
                // for the first few hundred milliseconds.
                let phase = frame % 20;
                if frame < 60 && phase < 10 {
                    pixels.fill(Rgb::RED);
                }
            }
        }
        pixels
    }

    /// Start the task that calls [`Self::render`] every [`RING_FRAME_MS`].
    ///
    /// The device loop only ever calls `set`, which is cheap; the animation
    /// runs on its own task so a slow upload or a blocking Wi-Fi join never
    /// freezes the ring. The task shares the pattern through a mutex.
    pub fn spawn_animation_task(&mut self) -> anyhow::Result<()> {
        todo!("spawn a FreeRTOS task rendering every {RING_FRAME_MS} ms")
    }

    /// Push one frame to the ring. Called from the animation task.
    pub fn render(&mut self) -> anyhow::Result<()> {
        let _pixels = self.frame_pixels();
        self.frame += 1;
        // FFI: rmt_transmit the GRB bytes, then rmt_tx_wait_all_done.
        todo!("transmit {RING_PIXELS} GRB triples over RMT")
    }
}

impl Ring for Ws2812Ring {
    fn set(&mut self, pattern: RingPattern) {
        if self.pattern != pattern {
            self.pattern = pattern;
            self.frame = 0;
        }
    }
}

/// A 0..=255 triangle wave with the given half-period in frames. Linear, not
/// sinusoidal: the difference is invisible through a diffuser.
fn triangle(frame: u64, half_period: u64) -> u16 {
    let position = frame % (half_period * 2);
    let rising = if position < half_period {
        position
    } else {
        half_period * 2 - position
    };
    ((rising * 255) / half_period) as u16
}
