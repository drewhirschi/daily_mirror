//! Press-length and click-sequence detection. Feed it the debounced button
//! level on every poll and act on the gestures it emits.

use crate::Millis;
use crate::timing::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// Released before 5 s.
    Short,
    /// Held past 2 s; emitted every poll until 5 s with the fill fraction.
    HoldProgress { percent: u8 },
    /// Held to 5 s. Emitted once while still held.
    LongPress,
    /// Held to 20 s. Emitted once while still held.
    ResetArmed,
    /// Reset was armed and the button was released; clicks are now counted.
    ResetClick { count: u8 },
    /// Three clicks within the window after a 20 s hold.
    FullReset,
    /// The click window expired without three clicks.
    ResetAborted,
}

#[derive(Debug, Default)]
pub struct GestureDetector {
    pressed_since: Option<Millis>,
    long_emitted: bool,
    reset_emitted: bool,
    reset_window: Option<ResetWindow>,
}

#[derive(Debug)]
struct ResetWindow {
    opened_at: Millis,
    clicks: u8,
    was_pressed: bool,
}

impl GestureDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call on every poll with the current debounced level.
    pub fn poll(&mut self, pressed: bool, now: Millis) -> Option<Gesture> {
        if let Some(window) = &mut self.reset_window {
            if now.saturating_sub(window.opened_at) > RESET_CLICK_WINDOW_MS {
                self.reset_window = None;
                return Some(Gesture::ResetAborted);
            }
            let edge = pressed && !window.was_pressed;
            window.was_pressed = pressed;
            if edge {
                window.clicks += 1;
                if window.clicks >= RESET_CLICK_COUNT {
                    self.reset_window = None;
                    return Some(Gesture::FullReset);
                }
                return Some(Gesture::ResetClick {
                    count: window.clicks,
                });
            }
            return None;
        }

        match (self.pressed_since, pressed) {
            (None, true) => {
                self.pressed_since = Some(now);
                self.long_emitted = false;
                self.reset_emitted = false;
                None
            }
            (Some(since), true) => {
                let held = now.saturating_sub(since);
                if held >= HOLD_RESET_MS && !self.reset_emitted {
                    self.reset_emitted = true;
                    Some(Gesture::ResetArmed)
                } else if held >= HOLD_PAIR_MS && !self.long_emitted {
                    self.long_emitted = true;
                    Some(Gesture::LongPress)
                } else if held >= HOLD_ARM_MS && !self.long_emitted {
                    let span = HOLD_PAIR_MS - HOLD_ARM_MS;
                    let percent = ((held - HOLD_ARM_MS) * 100 / span).min(100) as u8;
                    Some(Gesture::HoldProgress { percent })
                } else {
                    None
                }
            }
            (Some(since), false) => {
                let held = now.saturating_sub(since);
                self.pressed_since = None;
                if self.reset_emitted {
                    self.reset_window = Some(ResetWindow {
                        opened_at: now,
                        clicks: 0,
                        was_pressed: false,
                    });
                    None
                } else if held < HOLD_PAIR_MS {
                    Some(Gesture::Short)
                } else {
                    None
                }
            }
            (None, false) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(detector: &mut GestureDetector, steps: &[(u64, bool)]) -> Vec<Gesture> {
        steps
            .iter()
            .filter_map(|&(t, p)| detector.poll(p, Millis(t)))
            .collect()
    }

    #[test]
    fn short_press_emits_short_on_release() {
        let mut d = GestureDetector::new();
        assert_eq!(
            run(&mut d, &[(0, true), (100, true), (200, false)]),
            vec![Gesture::Short]
        );
    }

    #[test]
    fn hold_reports_progress_then_long_press_once() {
        let mut d = GestureDetector::new();
        let out = run(
            &mut d,
            &[
                (0, true),
                (1_000, true),
                (2_000, true),
                (3_500, true),
                (5_000, true),
                (6_000, true),
                (7_000, false),
            ],
        );
        assert_eq!(
            out,
            vec![
                Gesture::HoldProgress { percent: 0 },
                Gesture::HoldProgress { percent: 50 },
                Gesture::LongPress
            ]
        );
    }

    #[test]
    fn release_between_five_and_twenty_seconds_is_not_a_short_press() {
        let mut d = GestureDetector::new();
        let out = run(&mut d, &[(0, true), (5_000, true), (9_000, false)]);
        assert_eq!(out, vec![Gesture::LongPress]);
    }

    #[test]
    fn twenty_second_hold_then_triple_click_resets() {
        let mut d = GestureDetector::new();
        let mut steps = vec![(0, true), (5_000, true), (20_000, true), (20_100, false)];
        steps.extend([
            (20_500, true),
            (20_600, false),
            (21_000, true),
            (21_100, false),
            (21_500, true),
            (21_600, false),
        ]);
        let out = run(&mut d, &steps);
        assert_eq!(
            out,
            vec![
                Gesture::LongPress,
                Gesture::ResetArmed,
                Gesture::ResetClick { count: 1 },
                Gesture::ResetClick { count: 2 },
                Gesture::FullReset
            ]
        );
    }

    #[test]
    fn reset_window_expires_without_three_clicks() {
        let mut d = GestureDetector::new();
        let out = run(
            &mut d,
            &[
                (0, true),
                (5_000, true),
                (20_000, true),
                (20_100, false),
                (20_500, true),
                (20_600, false),
                (23_200, false),
            ],
        );
        assert_eq!(
            out,
            vec![
                Gesture::LongPress,
                Gesture::ResetArmed,
                Gesture::ResetClick { count: 1 },
                Gesture::ResetAborted
            ]
        );
    }

    #[test]
    fn a_click_after_an_aborted_reset_is_a_normal_short_press() {
        let mut d = GestureDetector::new();
        run(
            &mut d,
            &[
                (0, true),
                (5_000, true),
                (20_000, true),
                (20_100, false),
                (23_200, false),
            ],
        );
        assert_eq!(
            run(&mut d, &[(24_000, true), (24_100, false)]),
            vec![Gesture::Short]
        );
    }
}
