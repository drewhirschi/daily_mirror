//! The ring vocabulary. One logical RGB output; adapters render these patterns
//! on whatever hardware they have (three discrete LEDs, a NeoPixel ring, or a
//! terminal).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RingPattern {
    /// Unprovisioned: works offline, not yet paired.
    DimWhiteBreathe,
    /// Long-press in progress; `fraction` of the way from 2 s to 5 s.
    AmberFill {
        fraction_percent: u8,
    },
    /// Pairing: discoverable.
    AmberChase,
    /// Awaiting confirm: press the button to accept this phone.
    AmberFastPulse,
    /// Joining / claiming / uploading.
    BlueSlowPulse,
    /// Ready.
    SolidWhite,
    /// Countdown: three slow amber pulses.
    AmberSlowPulse,
    /// Capturing: hold still.
    AmberRapidPulse,
    /// The JPEG is committed.
    GreenFlash,
    /// Error or pairing failed; returns to the previous pattern afterwards.
    RedTriplePulse,
    /// Reset armed after a 20 s hold.
    SolidRed,
    /// Each click of the reset confirmation.
    RedFlash,
    Off,
}
