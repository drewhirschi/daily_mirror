//! Script mode: drive the whole pairing flow headlessly.
//!
//! A script is a text file of `<milliseconds> <action>` lines against a
//! virtual clock, so a five-minute pairing timeout costs nothing in a test.
//!
//! ```text
//! # first boot enters pairing on its own
//! 1000 credentials {"ssid":"home","psk":"secret","server_url":"http://127.0.0.1:3000","claim_token":"ct_1"}
//! 2000 click            # the confirming press
//! 3000 stop
//! ```
//!
//! | Action | Effect |
//! | --- | --- |
//! | `press` / `release` | move the button level |
//! | `click` | press and release 80 ms later |
//! | `hold <ms>` | press and release after `<ms>` — `hold 5000` enters pairing |
//! | `credentials <json>` | deliver credentials over the provisioning link |
//! | `stop` | end the run |

use anyhow::{Context, Result, anyhow, bail};
use daily_mirror_core::ports::ReceivedCredentials;
use daily_mirror_core::runtime::Step;
use daily_mirror_core::state::Event;
use daily_mirror_core::timing::BUTTON_POLL_MS;
use serde::Deserialize;

use crate::HostRuntime;
use crate::adapters::button::SharedButton;
use crate::adapters::clock::VirtualTime;

/// A short click, long enough to survive debouncing and short enough that the
/// gesture detector never calls it a hold.
pub const CLICK_MS: u64 = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Press,
    Release,
    Credentials(Box<ReceivedCredentials>),
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptStep {
    pub at_ms: u64,
    pub action: Action,
}

#[derive(Debug, Deserialize)]
struct CredentialsLine {
    ssid: String,
    psk: String,
    server_url: String,
    claim_token: String,
}

/// What a script run ended up doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// `Debug` of the final state, so callers can assert without matching on
    /// the core's private-ish payloads.
    pub final_state: String,
    /// A full reset ran to completion.
    pub rebooted: bool,
    pub ticks: u64,
}

/// Parse a script. Comments start with `#`; blank lines are ignored.
pub fn parse(text: &str) -> Result<Vec<ScriptStep>> {
    let mut steps = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = match line.split_once('#') {
            // A `#` inside a JSON payload would be unusual; strip comments only
            // when the line does not carry one.
            Some((before, _)) if !line.contains('{') => before,
            _ => line,
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        let (at, rest) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| anyhow!("line {}: expected `<milliseconds> <action>`", number + 1))?;
        let at_ms: u64 = at
            .parse()
            .with_context(|| format!("line {}: {at:?} is not a millisecond offset", number + 1))?;
        let rest = rest.trim();
        let (verb, argument) = match rest.split_once(char::is_whitespace) {
            Some((verb, argument)) => (verb, argument.trim()),
            None => (rest, ""),
        };
        match verb {
            "press" => steps.push(ScriptStep {
                at_ms,
                action: Action::Press,
            }),
            "release" => steps.push(ScriptStep {
                at_ms,
                action: Action::Release,
            }),
            "click" => {
                steps.push(ScriptStep {
                    at_ms,
                    action: Action::Press,
                });
                steps.push(ScriptStep {
                    at_ms: at_ms + CLICK_MS,
                    action: Action::Release,
                });
            }
            "hold" => {
                let millis: u64 = argument
                    .parse()
                    .with_context(|| format!("line {}: hold needs milliseconds", number + 1))?;
                steps.push(ScriptStep {
                    at_ms,
                    action: Action::Press,
                });
                steps.push(ScriptStep {
                    at_ms: at_ms + millis,
                    action: Action::Release,
                });
            }
            "credentials" => {
                let parsed: CredentialsLine =
                    serde_json::from_str(argument).with_context(|| {
                        format!("line {}: credentials needs a JSON object", number + 1)
                    })?;
                steps.push(ScriptStep {
                    at_ms,
                    action: Action::Credentials(Box::new(ReceivedCredentials {
                        ssid: parsed.ssid,
                        psk: parsed.psk,
                        server_url: parsed.server_url,
                        claim_token: parsed.claim_token,
                    })),
                });
            }
            "stop" => steps.push(ScriptStep {
                at_ms,
                action: Action::Stop,
            }),
            other => bail!("line {}: unknown action {other:?}", number + 1),
        }
    }
    steps.sort_by_key(|step| step.at_ms);
    Ok(steps)
}

/// Run a parsed script against a runtime built on a virtual clock. `settle_ms`
/// is how long the loop keeps ticking after the last step, so timeouts and
/// trailing work get a chance to happen.
pub fn run(
    runtime: &mut HostRuntime,
    time: &VirtualTime,
    button: &SharedButton,
    steps: &[ScriptStep],
    settle_ms: u64,
) -> Result<Outcome> {
    let end_ms = steps.last().map(|step| step.at_ms).unwrap_or(0) + settle_ms;
    let mut next = 0usize;
    let mut now = 0u64;
    let mut ticks = 0u64;
    let mut rebooted = false;

    time.set(0);
    if runtime.boot() == Step::Reboot {
        rebooted = true;
    }

    let mut stop = false;
    while !rebooted && !stop {
        time.set(now);
        while next < steps.len() && steps[next].at_ms <= now {
            match &steps[next].action {
                Action::Press => button.press(),
                Action::Release => button.release(),
                Action::Credentials(credentials) => {
                    if runtime.inject(Event::CredentialsReceived((**credentials).clone()))
                        == Step::Reboot
                    {
                        rebooted = true;
                    }
                }
                Action::Stop => stop = true,
            }
            next += 1;
        }
        if stop || rebooted {
            break;
        }
        if runtime.step() == Step::Reboot {
            rebooted = true;
            break;
        }
        ticks += 1;
        if now >= end_ms && next >= steps.len() {
            break;
        }
        now += BUTTON_POLL_MS;
    }

    Ok(Outcome {
        final_state: format!("{:?}", runtime.state()),
        rebooted,
        ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_becomes_a_press_and_a_release() {
        let steps = parse("100 click").unwrap();
        assert_eq!(
            steps,
            vec![
                ScriptStep {
                    at_ms: 100,
                    action: Action::Press
                },
                ScriptStep {
                    at_ms: 100 + CLICK_MS,
                    action: Action::Release
                },
            ]
        );
    }

    #[test]
    fn hold_spans_the_requested_duration() {
        let steps = parse("0 hold 5000").unwrap();
        assert_eq!(steps[0].at_ms, 0);
        assert_eq!(steps[1].at_ms, 5_000);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let steps = parse("# a comment\n\n10 press\n").unwrap();
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn credentials_survive_the_json_payload() {
        let steps = parse(
            r#"5 credentials {"ssid":"home","psk":"secret","server_url":"http://x","claim_token":"ct"}"#,
        )
        .unwrap();
        assert_eq!(
            steps[0].action,
            Action::Credentials(Box::new(ReceivedCredentials {
                ssid: "home".into(),
                psk: "secret".into(),
                server_url: "http://x".into(),
                claim_token: "ct".into(),
            }))
        );
    }

    #[test]
    fn unknown_actions_are_rejected_with_the_line_number() {
        let error = parse("0 press\n1 wiggle").unwrap_err().to_string();
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn steps_are_sorted_even_when_a_hold_overlaps_a_later_line() {
        let steps = parse("0 hold 5000\n1000 press").unwrap();
        let times: Vec<u64> = steps.iter().map(|step| step.at_ms).collect();
        assert_eq!(times, vec![0, 1_000, 5_000]);
    }
}
