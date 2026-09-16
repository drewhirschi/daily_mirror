//! The device state machine. Pure: `Machine::handle` takes an event and the
//! current time, mutates the state, and returns the commands the adapter must
//! execute. Every transition in `docs/device-pairing-plan.md` is covered by a
//! test below.

use crate::Millis;
use crate::button::Gesture;
use crate::ports::ReceivedCredentials;
use crate::ring::RingPattern;
use crate::timing::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No credentials. Offline capture works.
    Unprovisioned,
    /// Discoverable over the local link.
    Pairing {
        since: Millis,
    },
    /// Credentials arrived; waiting for the confirming press.
    AwaitingConfirm {
        since: Millis,
        credentials: ReceivedCredentials,
    },
    Joining {
        credentials: ReceivedCredentials,
    },
    Claiming {
        credentials: ReceivedCredentials,
    },
    Ready,
    Countdown {
        since: Millis,
        offline: bool,
    },
    Capturing {
        offline: bool,
    },
    Uploading,
    /// Held 20 s; clicks are being counted. `previous` is restored on abort.
    ResetArmed {
        previous: Box<State>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// First event after boot. `provisioned` is whether the store held credentials.
    Boot {
        provisioned: bool,
    },
    Gesture(Gesture),
    /// Periodic; drives timeouts. Adapters send one per button poll.
    Tick,
    /// The provisioning link delivered Wi-Fi credentials and the pairing payload.
    CredentialsReceived(ReceivedCredentials),
    WifiUp,
    WifiFailed,
    ClaimAccepted,
    ClaimRejected,
    CaptureCommitted,
    CaptureFailed,
    UploadSucceeded,
    UploadFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Ring(RingPattern),
    StartProvisioning,
    StopProvisioning,
    JoinWifi {
        ssid: String,
        psk: String,
    },
    /// Redeem the claim token at `server_url`; adapter replies with ClaimAccepted/Rejected.
    RedeemClaim {
        server_url: String,
        claim_token: String,
    },
    /// Persist the credentials and the token the adapter received from the claim.
    PersistProvisioned,
    StartFocus,
    Capture,
    /// Upload everything in the queue; adapter replies per outcome.
    DrainQueue,
    EraseAll,
    Reboot,
    /// Tell the app over the provisioning link that we are waiting for the press.
    ReportAwaitingConfirm,
    ReportClaimed,
    ReportFailed {
        reason: String,
    },
}

#[derive(Debug)]
pub struct Machine {
    state: State,
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: State::Unprovisioned,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn handle(&mut self, event: Event, now: Millis) -> Vec<Command> {
        use Command as C;
        use State as S;

        // Gestures that apply from any state.
        if let Event::Gesture(gesture) = &event {
            match gesture {
                Gesture::ResetArmed => {
                    let previous = Box::new(std::mem::replace(&mut self.state, S::Unprovisioned));
                    self.state = S::ResetArmed { previous };
                    return vec![C::Ring(RingPattern::SolidRed)];
                }
                Gesture::ResetClick { .. } => return vec![C::Ring(RingPattern::RedFlash)],
                Gesture::FullReset => {
                    self.state = S::Unprovisioned;
                    return vec![C::Ring(RingPattern::Off), C::EraseAll, C::Reboot];
                }
                Gesture::ResetAborted => {
                    if let S::ResetArmed { previous } =
                        std::mem::replace(&mut self.state, S::Unprovisioned)
                    {
                        self.state = *previous;
                    }
                    return vec![C::Ring(self.ring_for_state())];
                }
                Gesture::LongPress => {
                    if matches!(self.state, S::ResetArmed { .. }) {
                        return vec![];
                    }
                    return self.enter_pairing(now);
                }
                Gesture::HoldProgress { percent } => {
                    if matches!(self.state, S::ResetArmed { .. }) {
                        return vec![];
                    }
                    return vec![C::Ring(RingPattern::AmberFill {
                        fraction_percent: *percent,
                    })];
                }
                Gesture::Short => {}
            }
        }

        match (std::mem::replace(&mut self.state, S::Unprovisioned), event) {
            (S::Unprovisioned, Event::Boot { provisioned: false }) => self.enter_pairing(now),
            (S::Unprovisioned, Event::Boot { provisioned: true }) => {
                self.state = S::Ready;
                vec![C::Ring(RingPattern::SolidWhite), C::DrainQueue]
            }
            (S::Unprovisioned, Event::Gesture(Gesture::Short)) => self.start_countdown(now, true),
            (S::Unprovisioned, _) => {
                self.state = S::Unprovisioned;
                vec![]
            }

            (S::Pairing { since }, Event::Tick) => {
                if now.saturating_sub(since) >= PAIRING_TIMEOUT_MS {
                    self.state = S::Unprovisioned;
                    vec![C::StopProvisioning, C::Ring(RingPattern::DimWhiteBreathe)]
                } else {
                    self.state = S::Pairing { since };
                    vec![]
                }
            }
            (S::Pairing { .. }, Event::CredentialsReceived(credentials)) => {
                self.state = S::AwaitingConfirm {
                    since: now,
                    credentials,
                };
                vec![
                    C::Ring(RingPattern::AmberFastPulse),
                    C::ReportAwaitingConfirm,
                ]
            }
            (S::Pairing { since }, _) => {
                self.state = S::Pairing { since };
                vec![]
            }

            (S::AwaitingConfirm { credentials, .. }, Event::Gesture(Gesture::Short)) => {
                let join = C::JoinWifi {
                    ssid: credentials.ssid.clone(),
                    psk: credentials.psk.clone(),
                };
                self.state = S::Joining { credentials };
                vec![C::Ring(RingPattern::BlueSlowPulse), join]
            }
            (S::AwaitingConfirm { since, credentials }, Event::Tick) => {
                if now.saturating_sub(since) >= CONFIRM_TIMEOUT_MS {
                    self.state = S::Pairing { since: now };
                    vec![
                        C::Ring(RingPattern::AmberChase),
                        C::ReportFailed {
                            reason: "not confirmed".into(),
                        },
                    ]
                } else {
                    self.state = S::AwaitingConfirm { since, credentials };
                    vec![]
                }
            }
            (S::AwaitingConfirm { since, credentials }, _) => {
                self.state = S::AwaitingConfirm { since, credentials };
                vec![]
            }

            (S::Joining { credentials }, Event::WifiUp) => {
                let redeem = C::RedeemClaim {
                    server_url: credentials.server_url.clone(),
                    claim_token: credentials.claim_token.clone(),
                };
                self.state = S::Claiming { credentials };
                vec![redeem]
            }
            (S::Joining { .. }, Event::WifiFailed) => self.fail_pairing(now, "wifi join failed"),
            (S::Joining { credentials }, _) => {
                self.state = S::Joining { credentials };
                vec![]
            }

            (S::Claiming { .. }, Event::ClaimAccepted) => {
                self.state = S::Ready;
                vec![
                    C::PersistProvisioned,
                    C::ReportClaimed,
                    C::StopProvisioning,
                    C::Ring(RingPattern::SolidWhite),
                    C::DrainQueue,
                ]
            }
            (S::Claiming { .. }, Event::ClaimRejected) => self.fail_pairing(now, "claim rejected"),
            (S::Claiming { credentials }, _) => {
                self.state = S::Claiming { credentials };
                vec![]
            }

            (S::Ready, Event::Gesture(Gesture::Short)) => self.start_countdown(now, false),
            (S::Ready, Event::UploadSucceeded) | (S::Ready, Event::UploadFailed) => {
                self.state = S::Ready;
                vec![]
            }
            (S::Ready, _) => {
                self.state = S::Ready;
                vec![]
            }

            (S::Countdown { since, offline }, Event::Tick) => {
                if now.saturating_sub(since) >= COUNTDOWN_MS {
                    self.state = S::Capturing { offline };
                    vec![C::Ring(RingPattern::AmberRapidPulse), C::Capture]
                } else {
                    self.state = S::Countdown { since, offline };
                    vec![]
                }
            }
            (S::Countdown { since, offline }, _) => {
                self.state = S::Countdown { since, offline };
                vec![]
            }

            (S::Capturing { offline: true }, Event::CaptureCommitted) => {
                self.state = S::Unprovisioned;
                vec![
                    C::Ring(RingPattern::GreenFlash),
                    C::Ring(RingPattern::DimWhiteBreathe),
                ]
            }
            (S::Capturing { offline: false }, Event::CaptureCommitted) => {
                self.state = S::Uploading;
                vec![
                    C::Ring(RingPattern::GreenFlash),
                    C::Ring(RingPattern::BlueSlowPulse),
                    C::DrainQueue,
                ]
            }
            (S::Capturing { offline }, Event::CaptureFailed) => {
                self.state = if offline { S::Unprovisioned } else { S::Ready };
                vec![
                    C::Ring(RingPattern::RedTriplePulse),
                    C::Ring(self.ring_for_state()),
                ]
            }
            (S::Capturing { offline }, _) => {
                self.state = S::Capturing { offline };
                vec![]
            }

            (S::Uploading, Event::UploadSucceeded) => {
                self.state = S::Ready;
                vec![C::Ring(RingPattern::SolidWhite)]
            }
            (S::Uploading, Event::UploadFailed) => {
                self.state = S::Ready;
                vec![
                    C::Ring(RingPattern::RedTriplePulse),
                    C::Ring(RingPattern::SolidWhite),
                ]
            }
            (S::Uploading, _) => {
                self.state = S::Uploading;
                vec![]
            }

            (S::ResetArmed { previous }, _) => {
                self.state = S::ResetArmed { previous };
                vec![]
            }
        }
    }

    fn enter_pairing(&mut self, now: Millis) -> Vec<Command> {
        self.state = State::Pairing { since: now };
        vec![
            Command::StartProvisioning,
            Command::Ring(RingPattern::AmberChase),
        ]
    }

    fn fail_pairing(&mut self, now: Millis, reason: &str) -> Vec<Command> {
        self.state = State::Pairing { since: now };
        vec![
            Command::Ring(RingPattern::RedTriplePulse),
            Command::Ring(RingPattern::AmberChase),
            Command::ReportFailed {
                reason: reason.into(),
            },
        ]
    }

    fn start_countdown(&mut self, now: Millis, offline: bool) -> Vec<Command> {
        self.state = State::Countdown {
            since: now,
            offline,
        };
        vec![
            Command::Ring(RingPattern::AmberSlowPulse),
            Command::StartFocus,
        ]
    }

    /// The resting pattern for the current state.
    pub fn ring_for_state(&self) -> RingPattern {
        match &self.state {
            State::Unprovisioned => RingPattern::DimWhiteBreathe,
            State::Pairing { .. } => RingPattern::AmberChase,
            State::AwaitingConfirm { .. } => RingPattern::AmberFastPulse,
            State::Joining { .. } | State::Claiming { .. } | State::Uploading => {
                RingPattern::BlueSlowPulse
            }
            State::Ready => RingPattern::SolidWhite,
            State::Countdown { .. } => RingPattern::AmberSlowPulse,
            State::Capturing { .. } => RingPattern::AmberRapidPulse,
            State::ResetArmed { .. } => RingPattern::SolidRed,
        }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> ReceivedCredentials {
        ReceivedCredentials {
            ssid: "home".into(),
            psk: "secret".into(),
            server_url: "https://mirror.example".into(),
            claim_token: "ct_123".into(),
        }
    }

    fn claimed_machine() -> Machine {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        m.handle(Event::CredentialsReceived(creds()), Millis(1_000));
        m.handle(Event::Gesture(Gesture::Short), Millis(2_000));
        m.handle(Event::WifiUp, Millis(3_000));
        m.handle(Event::ClaimAccepted, Millis(4_000));
        assert_eq!(m.state(), &State::Ready);
        m
    }

    #[test]
    fn first_boot_without_credentials_enters_pairing() {
        let mut m = Machine::new();
        let out = m.handle(Event::Boot { provisioned: false }, Millis(0));
        assert_eq!(
            out,
            vec![
                Command::StartProvisioning,
                Command::Ring(RingPattern::AmberChase)
            ]
        );
        assert_eq!(m.state(), &State::Pairing { since: Millis(0) });
    }

    #[test]
    fn boot_with_credentials_is_ready_and_drains_queue() {
        let mut m = Machine::new();
        let out = m.handle(Event::Boot { provisioned: true }, Millis(0));
        assert_eq!(
            out,
            vec![Command::Ring(RingPattern::SolidWhite), Command::DrainQueue]
        );
        assert_eq!(m.state(), &State::Ready);
    }

    #[test]
    fn pairing_times_out_to_unprovisioned() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        assert!(
            m.handle(Event::Tick, Millis(PAIRING_TIMEOUT_MS - 1))
                .is_empty()
        );
        let out = m.handle(Event::Tick, Millis(PAIRING_TIMEOUT_MS));
        assert_eq!(
            out,
            vec![
                Command::StopProvisioning,
                Command::Ring(RingPattern::DimWhiteBreathe)
            ]
        );
        assert_eq!(m.state(), &State::Unprovisioned);
    }

    #[test]
    fn full_happy_path() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        let out = m.handle(Event::CredentialsReceived(creds()), Millis(1_000));
        assert_eq!(
            out,
            vec![
                Command::Ring(RingPattern::AmberFastPulse),
                Command::ReportAwaitingConfirm
            ]
        );
        let out = m.handle(Event::Gesture(Gesture::Short), Millis(2_000));
        assert_eq!(
            out,
            vec![
                Command::Ring(RingPattern::BlueSlowPulse),
                Command::JoinWifi {
                    ssid: "home".into(),
                    psk: "secret".into()
                }
            ]
        );
        let out = m.handle(Event::WifiUp, Millis(3_000));
        assert_eq!(
            out,
            vec![Command::RedeemClaim {
                server_url: "https://mirror.example".into(),
                claim_token: "ct_123".into()
            }]
        );
        let out = m.handle(Event::ClaimAccepted, Millis(4_000));
        assert_eq!(out[0], Command::PersistProvisioned);
        assert!(out.contains(&Command::ReportClaimed));
        assert!(out.contains(&Command::StopProvisioning));
        assert_eq!(m.state(), &State::Ready);
    }

    #[test]
    fn unconfirmed_credentials_expire_back_to_pairing() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        m.handle(Event::CredentialsReceived(creds()), Millis(1_000));
        let out = m.handle(Event::Tick, Millis(1_000 + CONFIRM_TIMEOUT_MS));
        assert!(out.contains(&Command::ReportFailed {
            reason: "not confirmed".into()
        }));
        assert!(matches!(m.state(), State::Pairing { .. }));
    }

    #[test]
    fn wifi_failure_and_claim_rejection_return_to_pairing() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        m.handle(Event::CredentialsReceived(creds()), Millis(1));
        m.handle(Event::Gesture(Gesture::Short), Millis(2));
        let out = m.handle(Event::WifiFailed, Millis(3));
        assert!(out.contains(&Command::ReportFailed {
            reason: "wifi join failed".into()
        }));
        assert!(matches!(m.state(), State::Pairing { .. }));

        m.handle(Event::CredentialsReceived(creds()), Millis(4));
        m.handle(Event::Gesture(Gesture::Short), Millis(5));
        m.handle(Event::WifiUp, Millis(6));
        let out = m.handle(Event::ClaimRejected, Millis(7));
        assert!(out.contains(&Command::ReportFailed {
            reason: "claim rejected".into()
        }));
        assert!(matches!(m.state(), State::Pairing { .. }));
    }

    #[test]
    fn short_press_while_unprovisioned_captures_offline_and_stays_unprovisioned() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        m.handle(Event::Tick, Millis(PAIRING_TIMEOUT_MS));
        let out = m.handle(
            Event::Gesture(Gesture::Short),
            Millis(PAIRING_TIMEOUT_MS + 1),
        );
        assert_eq!(
            out,
            vec![
                Command::Ring(RingPattern::AmberSlowPulse),
                Command::StartFocus
            ]
        );
        let out = m.handle(Event::Tick, Millis(PAIRING_TIMEOUT_MS + 1 + COUNTDOWN_MS));
        assert_eq!(
            out,
            vec![
                Command::Ring(RingPattern::AmberRapidPulse),
                Command::Capture
            ]
        );
        let out = m.handle(Event::CaptureCommitted, Millis(PAIRING_TIMEOUT_MS + 5_000));
        assert!(out.contains(&Command::Ring(RingPattern::GreenFlash)));
        assert!(!out.contains(&Command::DrainQueue));
        assert_eq!(m.state(), &State::Unprovisioned);
    }

    #[test]
    fn short_press_while_pairing_is_swallowed() {
        let mut m = Machine::new();
        m.handle(Event::Boot { provisioned: false }, Millis(0));
        assert!(
            m.handle(Event::Gesture(Gesture::Short), Millis(1))
                .is_empty()
        );
        assert!(matches!(m.state(), State::Pairing { .. }));
    }

    #[test]
    fn ready_capture_uploads_then_returns_to_ready() {
        let mut m = claimed_machine();
        m.handle(Event::Gesture(Gesture::Short), Millis(10_000));
        m.handle(Event::Tick, Millis(10_000 + COUNTDOWN_MS));
        let out = m.handle(Event::CaptureCommitted, Millis(14_000));
        assert!(out.contains(&Command::DrainQueue));
        assert_eq!(m.state(), &State::Uploading);
        let out = m.handle(Event::UploadFailed, Millis(15_000));
        assert_eq!(out[0], Command::Ring(RingPattern::RedTriplePulse));
        assert_eq!(m.state(), &State::Ready);
    }

    #[test]
    fn long_press_from_ready_repairs_without_erasing() {
        let mut m = claimed_machine();
        let out = m.handle(Event::Gesture(Gesture::LongPress), Millis(20_000));
        assert_eq!(
            out,
            vec![
                Command::StartProvisioning,
                Command::Ring(RingPattern::AmberChase)
            ]
        );
        assert!(!out.contains(&Command::EraseAll));
        assert_eq!(
            m.state(),
            &State::Pairing {
                since: Millis(20_000)
            }
        );
    }

    #[test]
    fn hold_progress_fills_ring() {
        let mut m = claimed_machine();
        let out = m.handle(
            Event::Gesture(Gesture::HoldProgress { percent: 40 }),
            Millis(1),
        );
        assert_eq!(
            out,
            vec![Command::Ring(RingPattern::AmberFill {
                fraction_percent: 40
            })]
        );
        assert_eq!(m.state(), &State::Ready);
    }

    #[test]
    fn full_reset_erases_and_reboots_from_any_state() {
        let mut m = claimed_machine();
        m.handle(Event::Gesture(Gesture::LongPress), Millis(1));
        let out = m.handle(Event::Gesture(Gesture::ResetArmed), Millis(2));
        assert_eq!(out, vec![Command::Ring(RingPattern::SolidRed)]);
        m.handle(Event::Gesture(Gesture::ResetClick { count: 1 }), Millis(3));
        m.handle(Event::Gesture(Gesture::ResetClick { count: 2 }), Millis(4));
        let out = m.handle(Event::Gesture(Gesture::FullReset), Millis(5));
        assert_eq!(
            out,
            vec![
                Command::Ring(RingPattern::Off),
                Command::EraseAll,
                Command::Reboot
            ]
        );
    }

    #[test]
    fn aborted_reset_restores_previous_state() {
        let mut m = claimed_machine();
        m.handle(Event::Gesture(Gesture::ResetArmed), Millis(1));
        assert!(matches!(m.state(), State::ResetArmed { .. }));
        let out = m.handle(Event::Gesture(Gesture::ResetAborted), Millis(2));
        assert_eq!(out, vec![Command::Ring(RingPattern::SolidWhite)]);
        assert_eq!(m.state(), &State::Ready);
    }

    #[test]
    fn network_events_are_ignored_while_reset_is_armed() {
        let mut m = claimed_machine();
        m.handle(Event::Gesture(Gesture::ResetArmed), Millis(1));
        assert!(m.handle(Event::UploadSucceeded, Millis(2)).is_empty());
        assert!(matches!(m.state(), State::ResetArmed { .. }));
    }
}
