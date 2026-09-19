//! The driver loop, shared by every platform.
//!
//! [`Runtime`] owns one adapter per trait in [`crate::ports`], the
//! [`Machine`], and the [`GestureDetector`]. It polls the button, polls the
//! provisioning link, feeds `Tick` and the resulting events into the machine,
//! and executes every [`Command`] the machine returns — turning each outcome
//! back into the event the machine expects.
//!
//! It is platform-neutral: it never sleeps, never reads a clock of its own,
//! and never touches the network except through [`Net`]. The platform's `main`
//! calls [`Runtime::boot`] once and then [`Runtime::step`] every
//! [`crate::timing::BUTTON_POLL_MS`] milliseconds, pacing itself however it
//! likes. That is the only difference between the host binary and the board.

use std::collections::VecDeque;

use crate::button::GestureDetector;
use crate::contract::{DeviceClaimRequest, DeviceClaimed, ProvisioningResult};
use crate::ports::{
    Button, Camera, Clock, Net, Provisioned, QueuedCapture, ReceivedCredentials, Ring, Store,
};
use crate::state::{Command, Event, Machine, State};

/// Where the runtime reports what it did. The host prints to the terminal; the
/// board writes to the ESP-IDF log.
pub trait Log {
    fn log(&mut self, message: &str);
}

/// Identity sent with the claim. `hardware` is the free-form board label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub device_id: String,
    pub firmware_version: String,
    pub hardware: String,
}

/// What the platform loop should do after a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// A full reset completed. The platform reboots (or, on host, exits).
    Reboot,
}

pub struct Runtime<K, B, R, C, S, N, L> {
    pub clock: K,
    pub button: B,
    pub ring: R,
    pub camera: C,
    pub store: S,
    pub net: N,
    pub log: L,
    identity: Identity,
    machine: Machine,
    detector: GestureDetector,
    queue: VecDeque<Event>,
    /// The last credentials delivered over the provisioning link. Needed at
    /// `PersistProvisioned`, which happens several transitions later.
    credentials: Option<ReceivedCredentials>,
    /// The claim response, held between `RedeemClaim` and `PersistProvisioned`.
    claimed: Option<DeviceClaimed>,
    provisioned: Option<Provisioned>,
    reboot: bool,
}

impl<K, B, R, C, S, N, L> Runtime<K, B, R, C, S, N, L>
where
    K: Clock,
    B: Button,
    R: Ring,
    C: Camera,
    S: Store,
    N: Net,
    L: Log,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: Identity,
        clock: K,
        button: B,
        ring: R,
        camera: C,
        store: S,
        net: N,
        log: L,
    ) -> Self {
        Self {
            clock,
            button,
            ring,
            camera,
            store,
            net,
            log,
            identity,
            machine: Machine::new(),
            detector: GestureDetector::new(),
            queue: VecDeque::new(),
            credentials: None,
            claimed: None,
            provisioned: None,
            reboot: false,
        }
    }

    pub fn state(&self) -> &State {
        self.machine.state()
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Load the store and send the `Boot` event.
    pub fn boot(&mut self) -> Step {
        self.provisioned = match self.store.load() {
            Ok(value) => value,
            Err(error) => {
                self.log(format!("store load failed: {error:?}"));
                None
            }
        };
        let provisioned = self.provisioned.is_some();
        self.log(format!("boot: provisioned={provisioned}"));
        self.dispatch(Event::Boot { provisioned })
    }

    /// One poll: button, provisioning link, tick.
    pub fn step(&mut self) -> Step {
        let now = self.clock.now();
        let pressed = self.button.is_pressed();
        if let Some(gesture) = self.detector.poll(pressed, now)
            && self.dispatch(Event::Gesture(gesture)) == Step::Reboot
        {
            return Step::Reboot;
        }
        if let Some(credentials) = self.net.poll_credentials()
            && self.dispatch(Event::CredentialsReceived(credentials)) == Step::Reboot
        {
            return Step::Reboot;
        }
        self.dispatch(Event::Tick)
    }

    /// Feed an event in from outside the adapters — used by the host's script
    /// mode and by tests.
    pub fn inject(&mut self, event: Event) -> Step {
        self.dispatch(event)
    }

    fn dispatch(&mut self, event: Event) -> Step {
        self.queue.push_back(event);
        while let Some(event) = self.queue.pop_front() {
            if let Event::CredentialsReceived(credentials) = &event {
                self.credentials = Some(credentials.clone());
            }
            let now = self.clock.now();
            let commands = self.machine.handle(event, now);
            for command in commands {
                self.run(command);
            }
        }
        if self.reboot {
            self.reboot = false;
            Step::Reboot
        } else {
            Step::Continue
        }
    }

    fn run(&mut self, command: Command) {
        match command {
            Command::Ring(pattern) => {
                self.log(format!("ring {pattern:?}"));
                self.ring.set(pattern);
            }
            Command::StartProvisioning => {
                let device_id = self.identity.device_id.clone();
                if let Err(error) = self.net.start_provisioning(&device_id) {
                    self.log(format!("start_provisioning failed: {error:?}"));
                }
            }
            Command::StopProvisioning => {
                if let Err(error) = self.net.stop_provisioning() {
                    self.log(format!("stop_provisioning failed: {error:?}"));
                }
            }
            Command::JoinWifi { ssid, psk } => match self.net.join(&ssid, &psk) {
                Ok(()) => self.queue.push_back(Event::WifiUp),
                Err(error) => {
                    self.log(format!("wifi join failed: {error:?}"));
                    self.queue.push_back(Event::WifiFailed);
                }
            },
            Command::RedeemClaim {
                server_url,
                claim_token,
            } => {
                let request = DeviceClaimRequest {
                    device_id: self.identity.device_id.clone(),
                    claim_token,
                    firmware_version: self.identity.firmware_version.clone(),
                    hardware: self.identity.hardware.clone(),
                };
                match self.net.claim(&server_url, &request) {
                    Ok(claimed) => {
                        self.log(format!("claimed as {}", claimed.device_name));
                        self.claimed = Some(claimed);
                        self.queue.push_back(Event::ClaimAccepted);
                    }
                    Err(error) => {
                        self.log(format!("claim rejected: {error:?}"));
                        self.queue.push_back(Event::ClaimRejected);
                    }
                }
            }
            Command::PersistProvisioned => match (self.credentials.clone(), self.claimed.clone()) {
                (Some(credentials), Some(claimed)) => {
                    let provisioned = Provisioned {
                        ssid: credentials.ssid,
                        psk: credentials.psk,
                        server_url: credentials.server_url,
                        device_token: claimed.device_token,
                        device_name: claimed.device_name,
                    };
                    if let Err(error) = self.store.save(&provisioned) {
                        self.log(format!("persist failed: {error:?}"));
                    } else {
                        self.provisioned = Some(provisioned);
                    }
                }
                _ => self.log("persist requested without credentials or a claim".into()),
            },
            Command::StartFocus => {
                if let Err(error) = self.camera.start_focus() {
                    self.log(format!("start_focus failed: {error:?}"));
                }
            }
            Command::Capture => match self.capture() {
                Ok(capture) => {
                    self.log(format!(
                        "queued {} ({} bytes)",
                        capture.capture_id, capture.bytes
                    ));
                    self.queue.push_back(Event::CaptureCommitted);
                }
                Err(message) => {
                    self.log(format!("capture failed: {message}"));
                    self.queue.push_back(Event::CaptureFailed);
                }
            },
            Command::DrainQueue => {
                let event = if self.drain() {
                    Event::UploadSucceeded
                } else {
                    Event::UploadFailed
                };
                self.queue.push_back(event);
            }
            Command::EraseAll => {
                if let Err(error) = self.store.erase_all() {
                    self.log(format!("erase failed: {error:?}"));
                }
                self.credentials = None;
                self.claimed = None;
                self.provisioned = None;
                match self.store.device_id() {
                    Ok(device_id) => self.identity.device_id = device_id,
                    Err(error) => self.log(format!("device id regeneration failed: {error:?}")),
                }
                self.log("erased all device state".into());
            }
            Command::Reboot => {
                self.reboot = true;
                self.queue.clear();
            }
            Command::ReportAwaitingConfirm => self.report(ProvisioningResult::AwaitingConfirm),
            Command::ReportClaimed => {
                let device_name = self
                    .claimed
                    .as_ref()
                    .map(|claimed| claimed.device_name.clone())
                    .unwrap_or_default();
                self.report(ProvisioningResult::Claimed { device_name });
            }
            Command::ReportFailed { reason } => self.report(ProvisioningResult::Failed { reason }),
        }
    }

    fn capture(&mut self) -> Result<QueuedCapture, String> {
        let jpeg = self
            .camera
            .capture()
            .map_err(|error| format!("{error:?}"))?;
        let capture_id = self
            .store
            .next_capture_id()
            .map_err(|error| format!("{error:?}"))?;
        self.store
            .enqueue(&capture_id, &jpeg)
            .map_err(|error| format!("{error:?}"))
    }

    /// Upload every queued capture. Returns false if anything was left behind.
    fn drain(&mut self) -> bool {
        let Some(provisioned) = self.provisioned.clone() else {
            self.log("drain requested before the device was provisioned".into());
            return false;
        };
        let pending = match self.store.pending() {
            Ok(pending) => pending,
            Err(error) => {
                self.log(format!("queue listing failed: {error:?}"));
                return false;
            }
        };
        let mut ok = true;
        for capture in pending {
            let jpeg = match self.store.read(&capture.capture_id) {
                Ok(jpeg) => jpeg,
                Err(error) => {
                    self.log(format!("queue read failed: {error:?}"));
                    ok = false;
                    continue;
                }
            };
            match self.net.upload(
                &provisioned.server_url,
                &provisioned.device_token,
                &capture,
                &jpeg,
            ) {
                Ok(()) => {
                    if let Err(error) = self.store.remove(&capture.capture_id) {
                        self.log(format!("queue removal failed: {error:?}"));
                        ok = false;
                    } else {
                        self.log(format!("uploaded {}", capture.capture_id));
                    }
                }
                Err(error) => {
                    self.log(format!(
                        "upload deferred for {}: {error:?}",
                        capture.capture_id
                    ));
                    ok = false;
                }
            }
        }
        ok
    }

    fn report(&mut self, result: ProvisioningResult) {
        if let Err(error) = self.net.report(&result) {
            self.log(format!("provisioning report failed: {error:?}"));
        }
    }

    fn log(&mut self, message: String) {
        self.log.log(&message);
    }
}
