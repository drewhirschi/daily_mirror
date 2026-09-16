//! Net: hosted Wi-Fi through the companion C6, SoftAP provisioning, the
//! claim, and the upload.
//!
//! Three C libraries meet here:
//!
//! - **esp_wifi_remote over esp_hosted.** The P4 has no radio. The familiar
//!   `esp_wifi_*` API is forwarded over SDIO to the C6, so `join` looks like
//!   ordinary ESP-IDF code and behaves like ordinary ESP-IDF code.
//! - **network_provisioning, SoftAP scheme.** The phone joins the device's
//!   access point, runs the Curve25519 handshake with proof of possession,
//!   sends Wi-Fi credentials in the standard `wifi_config` step, and sends
//!   `server_url` and `claim_token` on the custom `daily-mirror` endpoint.
//!   BLE is the better first-run experience and is a one-line scheme swap
//!   once upstream finishes it for the P4.
//! - **esp_http_client with the certificate bundle.** The claim and the
//!   grant / PUT / complete upload, over TLS.
//!
//! The provisioning callback runs on the provisioning task, not the device
//! loop, so credentials land in a FreeRTOS queue and `poll_credentials` drains
//! it from the loop. That is the same shape as the host adapter's inbox.

use anyhow::Result;
use daily_mirror_core::contract::{DeviceClaimRequest, DeviceClaimed, ProvisioningResult};
use daily_mirror_core::ports::{Net, QueuedCapture, ReceivedCredentials};

/// SoftAP SSID prefix. The device id's last six characters are appended, so
/// two devices pairing side by side are distinguishable in the phone's Wi-Fi
/// list: `DailyMirror-3f9c21`.
pub const SOFTAP_SSID_PREFIX: &str = "DailyMirror-";
/// Custom provisioning endpoint carrying the claim payload.
pub const PROVISIONING_ENDPOINT: &str = "daily-mirror";
/// Proof of possession for the provisioning handshake. Derived from the device
/// id so the app can compute it from what it discovers, and re-derived after a
/// full reset because the device id changes.
pub const POP_LENGTH: usize = 8;

pub struct HostedNet {
    // A `QueueHandle_t` of credentials filled by the provisioning callback,
    // the `esp_netif` handles for the station and the SoftAP, and the
    // `esp_http_client` configuration.
}

impl HostedNet {
    /// Bring up the hosted transport and the network interfaces. Runs after
    /// the button and the ring, because it is the slowest thing on the board
    /// and the ring has to be showing something while it happens.
    pub fn new() -> Result<Self> {
        // FFI: esp_netif_init, esp_event_loop_create_default,
        // esp_hosted_init, esp_wifi_remote_init with a default config, and
        // register the event handler that turns IP_EVENT_STA_GOT_IP into the
        // signal `join` waits on.
        todo!("initialize esp_hosted and esp_wifi_remote")
    }
}

impl Net for HostedNet {
    type Error = anyhow::Error;

    fn start_provisioning(&mut self, _device_id: &str) -> Result<()> {
        // FFI: network_prov_mgr_init with scheme_softap,
        // network_prov_mgr_endpoint_create(PROVISIONING_ENDPOINT),
        // network_prov_mgr_start_provisioning with SECURITY_1 and the
        // device-derived proof of possession, then
        // network_prov_mgr_endpoint_register with a handler that parses
        // `ProvisioningPayload` and pushes onto the credentials queue.
        //
        // The Wi-Fi credentials arrive separately in the standard wifi_config
        // step; hold them until the custom endpoint delivers the claim token,
        // then push one `ReceivedCredentials` with all four fields.
        todo!("start SoftAP provisioning as {SOFTAP_SSID_PREFIX}<suffix>")
    }

    fn stop_provisioning(&mut self) -> Result<()> {
        // network_prov_mgr_stop_provisioning then network_prov_mgr_deinit.
        // Called once the claim succeeds, which also tears down the SoftAP.
        todo!("stop the provisioning manager")
    }

    fn poll_credentials(&mut self) -> Option<ReceivedCredentials> {
        // xQueueReceive with a zero timeout: the device loop must never block.
        todo!("drain the provisioning queue without blocking")
    }

    fn report(&mut self, _result: &ProvisioningResult) -> Result<()> {
        // The app polls the custom endpoint; store the serialized
        // `ProvisioningResult` for the handler to return. This is how the
        // phone learns to show "press the button now" and, later, the device
        // name the server assigned.
        todo!("publish the provisioning result on the custom endpoint")
    }

    fn join(&mut self, _ssid: &str, _psk: &str) -> Result<()> {
        // esp_wifi_remote_set_config + esp_wifi_remote_connect, then wait on
        // the got-IP event with a timeout. The core treats the error as
        // WifiFailed and returns to pairing, so a wrong password is a
        // recoverable, retryable mistake rather than a wedged device.
        todo!("join the network over the hosted radio and wait for an IP")
    }

    fn is_connected(&mut self) -> bool {
        todo!("report whether the station interface holds an IP")
    }

    fn claim(&mut self, _server_url: &str, _request: &DeviceClaimRequest) -> Result<DeviceClaimed> {
        // POST {server_url}/api/devices/claim with the JSON body, no
        // authentication — the claim token is the credential. Parse
        // `DeviceClaimed` out of the response.
        todo!("POST the claim and decode the device token")
    }

    fn upload(
        &mut self,
        _server_url: &str,
        _device_token: &str,
        _capture: &QueuedCapture,
        _jpeg: &[u8],
    ) -> Result<()> {
        // The Pi's protocol, unchanged except that the bearer is now this
        // device's own token:
        //
        // 1. POST {server_url}/api/uploads with {capture_id, content_type,
        //    content_length} and the bearer token; receive {method, url,
        //    headers, complete_url}.
        // 2. PUT the JPEG to the returned URL with the returned headers. Send
        //    the bearer only when the target is the server's own origin — the
        //    R2 signed URL must not carry it.
        // 3. POST complete_url when present.
        //
        // Stream from the SD card rather than buffering: a 1080p JPEG plus
        // TLS buffers is a lot of PSRAM to hold at once.
        todo!("run the grant, PUT and complete sequence")
    }
}
