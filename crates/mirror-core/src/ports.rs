//! Adapter traits. The core calls nothing itself; an adapter loop reads
//! events from these, feeds them to [`crate::state::Machine`], and executes
//! the returned commands through them.

use crate::Millis;
use crate::ring::RingPattern;

/// Wi-Fi credentials plus the pairing payload received over the local link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedCredentials {
    pub ssid: String,
    pub psk: String,
    pub server_url: String,
    pub claim_token: String,
}

/// Everything a claimed device persists. Erased by a full reset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provisioned {
    pub ssid: String,
    pub psk: String,
    pub server_url: String,
    pub device_token: String,
    pub device_name: String,
}

pub trait Clock {
    fn now(&self) -> Millis;
}

pub trait Button {
    /// True while the button is physically held. Adapters debounce.
    fn is_pressed(&mut self) -> bool;
}

pub trait Ring {
    fn set(&mut self, pattern: RingPattern);
}

/// A captured JPEG that has been durably queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedCapture {
    pub capture_id: String,
    pub bytes: u64,
}

pub trait Camera {
    type Error: core::fmt::Debug;
    /// Start focus tracking; called at the start of the countdown.
    fn start_focus(&mut self) -> Result<(), Self::Error>;
    /// Take the full frame and return the JPEG bytes.
    fn capture(&mut self) -> Result<Vec<u8>, Self::Error>;
}

pub trait Store {
    type Error: core::fmt::Debug;
    fn device_id(&mut self) -> Result<String, Self::Error>;
    fn load(&mut self) -> Result<Option<Provisioned>, Self::Error>;
    fn save(&mut self, provisioned: &Provisioned) -> Result<(), Self::Error>;
    /// Mint the identifier for the next capture. The store owns the naming
    /// scheme because it also owns the queue's file layout.
    fn next_capture_id(&mut self) -> Result<String, Self::Error>;
    fn enqueue(&mut self, capture_id: &str, jpeg: &[u8]) -> Result<QueuedCapture, Self::Error>;
    /// Read a queued JPEG back for upload.
    fn read(&mut self, capture_id: &str) -> Result<Vec<u8>, Self::Error>;
    fn pending(&mut self) -> Result<Vec<QueuedCapture>, Self::Error>;
    fn remove(&mut self, capture_id: &str) -> Result<(), Self::Error>;
    /// Erase credentials, token, queue and settings, and regenerate the device id.
    fn erase_all(&mut self) -> Result<(), Self::Error>;
}

pub trait Net {
    type Error: core::fmt::Debug;
    fn start_provisioning(&mut self, device_id: &str) -> Result<(), Self::Error>;
    fn stop_provisioning(&mut self) -> Result<(), Self::Error>;
    /// Non-blocking: the credentials the app delivered over the provisioning
    /// link, if any have arrived since the last poll.
    fn poll_credentials(&mut self) -> Option<ReceivedCredentials>;
    /// Report pairing progress back over the provisioning link.
    fn report(&mut self, result: &crate::contract::ProvisioningResult) -> Result<(), Self::Error>;
    fn join(&mut self, ssid: &str, psk: &str) -> Result<(), Self::Error>;
    fn is_connected(&mut self) -> bool;
    fn claim(
        &mut self,
        server_url: &str,
        request: &crate::contract::DeviceClaimRequest,
    ) -> Result<crate::contract::DeviceClaimed, Self::Error>;
    fn upload(
        &mut self,
        server_url: &str,
        device_token: &str,
        capture: &QueuedCapture,
        jpeg: &[u8],
    ) -> Result<(), Self::Error>;
}
