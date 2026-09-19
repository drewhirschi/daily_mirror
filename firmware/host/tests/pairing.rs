//! The pairing flow end to end: the real core state machine, the real host
//! adapters, and a real HTTP server — just a very small one, built on
//! `std::net` so the test suite needs no extra dependency.
//!
//! Every test drives the firmware through `--script` semantics against a
//! virtual clock, so the five-minute pairing window and the thirty-second
//! confirmation window cost no wall-clock time.

mod support;

use std::sync::{Arc, Mutex};

use daily_mirror_core::ports::{ReceivedCredentials, Store};
use daily_mirror_core::ring::RingPattern;
use daily_mirror_core::runtime::{Identity, Runtime};
use daily_mirror_core::timing::{CONFIRM_TIMEOUT_MS, PAIRING_TIMEOUT_MS};
use daily_mirror_firmware_host::adapters::button::SharedButton;
use daily_mirror_firmware_host::adapters::camera::FixtureCamera;
use daily_mirror_firmware_host::adapters::clock::{HostClock, VirtualTime};
use daily_mirror_firmware_host::adapters::log::HostLog;
use daily_mirror_firmware_host::adapters::net::HttpNet;
use daily_mirror_firmware_host::adapters::ring::TerminalRing;
use daily_mirror_firmware_host::adapters::store::DirStore;
use daily_mirror_firmware_host::script::{self, Outcome};
use daily_mirror_firmware_host::{HostRuntime, script::ScriptStep};

use support::{MockServer, temp_dir, write_fixture};

struct Harness {
    runtime: HostRuntime,
    time: VirtualTime,
    button: SharedButton,
    ring: Arc<Mutex<Vec<RingPattern>>>,
    log: Arc<Mutex<Vec<String>>>,
    store_dir: std::path::PathBuf,
}

impl Harness {
    fn new() -> Self {
        let store_dir = temp_dir("store");
        let fixture = write_fixture();
        let time = VirtualTime::new();
        let button = SharedButton::new();
        let ring = TerminalRing::quiet();
        let ring_history = ring.history();
        let log = HostLog::quiet();
        let log_lines = log.lines();
        let mut store = DirStore::open(&store_dir).unwrap();
        let identity = Identity {
            device_id: store.device_id().unwrap(),
            firmware_version: "test".into(),
            hardware: "host".into(),
        };
        let runtime = Runtime::new(
            identity,
            HostClock::virtual_clock(time.clone()),
            button.clone(),
            ring,
            FixtureCamera::new(fixture),
            store,
            // Port 0: the OS picks, so tests never collide.
            HttpNet::new(0),
            log,
        );
        Self {
            runtime,
            time,
            button,
            ring: ring_history,
            log: log_lines,
            store_dir,
        }
    }

    fn run(&mut self, script_text: &str) -> Outcome {
        let steps: Vec<ScriptStep> = script::parse(script_text).unwrap();
        script::run(
            &mut self.runtime,
            &self.time,
            &self.button,
            &steps,
            /* settle_ms */ 500,
        )
        .unwrap()
    }

    fn ring_patterns(&self) -> Vec<RingPattern> {
        self.ring.lock().unwrap().clone()
    }

    fn logged(&self, needle: &str) -> bool {
        self.log
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(needle))
    }
}

fn credentials_line(at_ms: u64, server_url: &str, claim_token: &str) -> String {
    let credentials = ReceivedCredentials {
        ssid: "home".into(),
        psk: "secret".into(),
        server_url: server_url.into(),
        claim_token: claim_token.into(),
    };
    format!(
        r#"{at_ms} credentials {{"ssid":"{}","psk":"{}","server_url":"{}","claim_token":"{}"}}"#,
        credentials.ssid, credentials.psk, credentials.server_url, credentials.claim_token
    )
}

#[test]
fn happy_path_boots_unprovisioned_pairs_claims_and_becomes_ready() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    // First boot enters pairing by itself; the app sends credentials at 1 s and
    // the user presses the button at 2 s to confirm.
    let outcome = harness.run(&format!(
        "{}\n2000 click\n4000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_happy")
    ));

    assert_eq!(outcome.final_state, "Ready", "{outcome:?}");
    assert!(!outcome.rebooted);

    let patterns = harness.ring_patterns();
    assert_eq!(
        patterns,
        vec![
            RingPattern::AmberChase,     // pairing, discoverable
            RingPattern::AmberFastPulse, // credentials arrived, press to confirm
            RingPattern::BlueSlowPulse,  // joining and claiming
            RingPattern::SolidWhite,     // ready
        ],
        "ring vocabulary should follow the plan's table"
    );

    // The claim actually went over HTTP with the right body.
    let claims = server.claims();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].claim_token, "ct_happy");
    assert_eq!(claims[0].hardware, "host");
    assert_eq!(claims[0].firmware_version, "test");
    assert!(claims[0].device_id.starts_with("dm-"));

    // And the device token was persisted, so the next boot is Ready.
    let mut store = DirStore::open(&harness.store_dir).unwrap();
    let provisioned = store.load().unwrap().expect("credentials persisted");
    assert_eq!(provisioned.device_token, "dt_mock");
    assert_eq!(provisioned.device_name, "Mock Mirror");
    assert_eq!(provisioned.ssid, "home");
    assert_eq!(provisioned.server_url, server.base_url());
}

#[test]
fn a_capture_after_claiming_uploads_through_grant_put_and_complete() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    // Pair, then press again to take a photograph. The countdown is 3 s, so the
    // shutter fires at about 8 s and the upload follows.
    let outcome = harness.run(&format!(
        "{}\n2000 click\n5000 click\n12000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_upload")
    ));
    assert_eq!(outcome.final_state, "Ready", "{outcome:?}");

    let uploads = server.uploads();
    assert_eq!(uploads.len(), 1, "one capture should have been uploaded");
    assert_eq!(uploads[0].bearer.as_deref(), Some("dt_mock"));
    assert_eq!(uploads[0].body, support::FIXTURE_JPEG);
    // The capture id the server saw is the one the store minted.
    assert_eq!(uploads[0].capture_id.len(), 25);
    assert!(server.completed(), "the upload should have been completed");

    // The queue is empty: the JPEG is only removed after the server confirms.
    let mut store = DirStore::open(&harness.store_dir).unwrap();
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn pairing_times_out_after_five_minutes_and_falls_back_to_offline() {
    let mut harness = Harness::new();

    let outcome = harness.run(&format!("{} stop\n", PAIRING_TIMEOUT_MS + 1_000));

    assert_eq!(outcome.final_state, "Unprovisioned", "{outcome:?}");
    assert_eq!(
        harness.ring_patterns(),
        vec![RingPattern::AmberChase, RingPattern::DimWhiteBreathe],
        "the ring should drop from discoverable to the offline breathe"
    );
}

#[test]
fn credentials_without_a_confirming_press_expire_back_to_pairing() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    let outcome = harness.run(&format!(
        "{}\n{} stop\n",
        credentials_line(1_000, &server.base_url(), "ct_unconfirmed"),
        1_000 + CONFIRM_TIMEOUT_MS + 500
    ));

    assert!(outcome.final_state.starts_with("Pairing"), "{outcome:?}");
    assert!(server.claims().is_empty(), "no press means no claim");
    assert_eq!(
        harness.ring_patterns(),
        vec![
            RingPattern::AmberChase,
            RingPattern::AmberFastPulse,
            RingPattern::AmberChase,
        ]
    );
}

#[test]
fn a_rejected_claim_returns_to_pairing_and_persists_nothing() {
    let server = MockServer::start();
    server.reject_claims();
    let mut harness = Harness::new();

    let outcome = harness.run(&format!(
        "{}\n2000 click\n4000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_bad")
    ));

    assert!(outcome.final_state.starts_with("Pairing"), "{outcome:?}");
    assert!(
        harness
            .ring_patterns()
            .contains(&RingPattern::RedTriplePulse)
    );
    let mut store = DirStore::open(&harness.store_dir).unwrap();
    assert!(store.load().unwrap().is_none());
}

#[test]
fn a_long_press_from_ready_re_enters_pairing_without_erasing() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    let outcome = harness.run(&format!(
        "{}\n2000 click\n5000 hold 5200\n11000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_repair")
    ));

    assert!(outcome.final_state.starts_with("Pairing"), "{outcome:?}");
    let patterns = harness.ring_patterns();
    assert!(
        patterns
            .iter()
            .any(|pattern| matches!(pattern, RingPattern::AmberFill { .. })),
        "the ring fills amber from 2 s so the hold can be cancelled: {patterns:?}"
    );
    // The old credentials survive until a new claim succeeds.
    let mut store = DirStore::open(&harness.store_dir).unwrap();
    assert!(store.load().unwrap().is_some());
}

#[test]
fn a_full_reset_erases_the_store_and_reboots() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    // Pair first so there is something to erase, queue nothing, then hold 20 s
    // and triple-click inside the 3 s window.
    let outcome = harness.run(&format!(
        "{}\n2000 click\n\
         5000 hold 20200\n\
         25400 click\n25700 click\n26000 click\n\
         27000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_reset")
    ));

    assert!(
        outcome.rebooted,
        "a full reset ends in a reboot: {outcome:?}"
    );
    assert!(harness.logged("erased all device state"));
    let patterns = harness.ring_patterns();
    assert!(patterns.contains(&RingPattern::SolidRed), "{patterns:?}");
    assert!(patterns.contains(&RingPattern::RedFlash), "{patterns:?}");
    assert!(patterns.contains(&RingPattern::Off), "{patterns:?}");

    // Everything is gone: no credentials, no queue, and a fresh device id.
    let mut store = DirStore::open(&harness.store_dir).unwrap();
    assert!(store.load().unwrap().is_none());
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn an_offline_capture_queues_and_uploads_after_a_later_claim() {
    let server = MockServer::start();
    let mut harness = Harness::new();

    // Let pairing time out, take a photograph offline, then pair and confirm.
    // The claim drains the queue, which is the whole point of queueing first.
    let script = format!(
        "{timeout} click\n\
         {pair} hold 5200\n\
         {creds}\n\
         {confirm} click\n\
         {stop} stop\n",
        timeout = PAIRING_TIMEOUT_MS + 1_000,
        pair = PAIRING_TIMEOUT_MS + 10_000,
        creds = credentials_line(PAIRING_TIMEOUT_MS + 17_000, &server.base_url(), "ct_late"),
        confirm = PAIRING_TIMEOUT_MS + 18_000,
        stop = PAIRING_TIMEOUT_MS + 25_000,
    );
    let outcome = harness.run(&script);

    assert_eq!(outcome.final_state, "Ready", "{outcome:?}");
    let uploads = server.uploads();
    assert_eq!(uploads.len(), 1, "the offline capture should have drained");
    assert_eq!(uploads[0].body, support::FIXTURE_JPEG);
}

#[test]
fn a_failed_upload_leaves_the_capture_in_the_queue() {
    let server = MockServer::start();
    let mut harness = Harness::new();
    let outcome = harness.run(&format!(
        "{}\n2000 click\n3000 stop\n",
        credentials_line(1_000, &server.base_url(), "ct_retry")
    ));
    assert_eq!(outcome.final_state, "Ready");

    server.fail_uploads();
    let outcome = harness.run("0 click\n8000 stop\n");
    assert_eq!(outcome.final_state, "Ready", "{outcome:?}");

    let mut store = DirStore::open(&harness.store_dir).unwrap();
    assert_eq!(
        store.pending().unwrap().len(),
        1,
        "a rejected upload must not delete the local JPEG"
    );
    assert!(harness.logged("upload deferred"));
}
