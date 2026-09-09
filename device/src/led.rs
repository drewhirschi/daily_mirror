use anyhow::{Result, bail};

/// Logical green/yellow/red statuses can drive the original LEDs or one RGB LED.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedMode {
    Discrete,
    RgbCommonAnode,
}

impl LedMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "discrete" => Ok(Self::Discrete),
            "rgb-common-anode" => Ok(Self::RgbCommonAnode),
            _ => bail!("DAILY_MIRROR_LED_MODE must be discrete or rgb-common-anode"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Discrete => "discrete",
            Self::RgbCommonAnode => "rgb-common-anode",
        }
    }

    /// Electrical HIGH levels in green / yellow-or-blue / red pin order.
    pub fn levels(self, green: bool, yellow: bool, red: bool) -> [bool; 3] {
        match self {
            Self::Discrete => [green, yellow, red],
            Self::RgbCommonAnode => [!(green || yellow), true, !(red || yellow)],
        }
    }

    pub fn high_duty_cycle(self, brightness: f64) -> f64 {
        match self {
            Self::Discrete => brightness,
            Self::RgbCommonAnode => 1.0 - brightness,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LedMode;

    #[test]
    fn rgb_statuses_have_correct_channels_and_active_low_polarity() {
        let mode = LedMode::RgbCommonAnode;
        assert_eq!(mode.levels(false, false, false), [true, true, true]);
        assert_eq!(mode.levels(true, false, false), [false, true, true]);
        assert_eq!(mode.levels(false, true, false), [false, true, false]);
        assert_eq!(mode.levels(false, false, true), [true, true, false]);
        assert_eq!(mode.high_duty_cycle(0.0), 1.0);
        assert_eq!(mode.high_duty_cycle(1.0), 0.0);
        assert!((mode.high_duty_cycle(0.25) - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn original_hardware_keeps_all_eight_output_combinations() {
        for g in [false, true] {
            for y in [false, true] {
                for r in [false, true] {
                    assert_eq!(LedMode::Discrete.levels(g, y, r), [g, y, r]);
                }
            }
        }
        assert_eq!(LedMode::Discrete.high_duty_cycle(0.25), 0.25);
    }

    #[test]
    fn rejects_unknown_wiring_modes() {
        assert!(LedMode::parse("common-cathode").is_err());
        assert_eq!(LedMode::parse("discrete").unwrap(), LedMode::Discrete);
        assert_eq!(
            LedMode::parse("rgb-common-anode").unwrap(),
            LedMode::RgbCommonAnode
        );
    }
}
