/*
 * Device pairing: the app's "Add a mirror" flow, device side.
 *
 * The protocol is Espressif's standard Wi-Fi provisioning over BLE, which is
 * what mobile/src/pairing/provisioning.ts drives through the ESPProvision SDK.
 * Five things have to match the app exactly:
 *
 *   transport      BLE (NimBLE here). The phone scans Bluetooth and shows the
 *                  device in a list; nobody has to join a Wi-Fi network to set
 *                  a camera up
 *   device name    "mirror-<last 3 MAC bytes>" - the app scans for the
 *                  "mirror-" prefix, so the case matters
 *   service UUID   the provisioning default, 0000ffff-0000-1000-8000-
 *                  00805f9b34fb, which the ESPProvision SDKs already know
 *   security       protocomm security 2 (SRP6a). The salt and verifier are
 *                  generated at start-up from CONFIG_MIRROR_PROV_USERNAME and
 *                  CONFIG_MIRROR_PROV_POP
 *   endpoint       "daily-mirror", carrying {server_url, claim_token} in and
 *                  a ProvisioningResult back out:
 *                  {"status":"awaiting_confirm"} |
 *                  {"status":"claimed","device_name":...} |
 *                  {"status":"failed","reason":...}
 *
 * The state machine is the one in crates/mirror-core/src/state.rs, with its
 * timings (5 min pairing window, 30 s confirmation window):
 *
 *   Pairing -- payload --> AwaitingConfirm -- short press --> Claiming
 *      ^                        |                               |
 *      |                     30 s, no press                     | claim fails
 *      +------------------------+---------------------------<---+
 *
 *   Claiming -- device token --> Ready (persisted, mDNS claimed, ring white)
 *
 * Unlike the host adapter, the Wi-Fi join is not a step this module drives:
 * the provisioning manager joins as part of the standard wifi_config exchange,
 * because the app's provision() call waits on that result. The confirming
 * press therefore gates the claim - the trust-bearing hop, where a short-lived
 * token becomes this device's long-lived one - rather than the join.
 *
 * The Bluetooth controller's memory is released once pairing is over, because
 * the camera, the Wi-Fi stack and a TLS handshake all want the internal RAM it
 * was holding. That memory cannot be reclaimed without a restart, so a
 * long-press after pairing has ended reboots into pairing rather than trying
 * to raise the BLE stack again - see mirror_pair_start().
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"

#include "mirror_ring.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    MIRROR_PAIR_IDLE = 0,       /* not pairing: Ready, or Unprovisioned */
    MIRROR_PAIR_PAIRING,        /* discoverable; the app can connect */
    MIRROR_PAIR_AWAITING_CONFIRM, /* payload received; waiting for the press */
    MIRROR_PAIR_CLAIMING,       /* pressed; talking to the server */
} mirror_pair_state_t;

/** True when a device token is stored, i.e. a household owns this device. */
bool mirror_pair_claimed(void);

/**
 * Enter pairing. Safe to call from any state and more than once: an already
 * running service simply has its five-minute window restarted.
 *
 * If the Bluetooth controller has already been released, this arms pairing for
 * the next boot and restarts the device, which is the only way to get the BLE
 * stack back. Either way it returns ESP_OK.
 *
 * Existing Wi-Fi credentials and the existing device token are left alone.
 * They are replaced only when a new claim succeeds.
 */
esp_err_t mirror_pair_start(void);

/**
 * True when this boot should enter pairing: no device token stored, or a
 * long-press asked for it before the last restart. Call after
 * mirror_config_init(); it clears the one-shot flag.
 */
bool mirror_pair_wanted_at_boot(void);

mirror_pair_state_t mirror_pair_state(void);

/**
 * True while pairing is using the button, so the press flow must not capture.
 * (The plan: "Only Pairing itself swallows the short press, because it needs
 * it for confirmation.")
 */
bool mirror_pair_busy(void);

/**
 * Offer the confirming short press. Returns true if pairing consumed it, in
 * which case the caller must not run the capture flow.
 */
bool mirror_pair_confirm(void);

/** The pattern the ring should show for whatever state the device is in. */
ring_pattern_t mirror_pair_ring(void);

/** Erase every stored setting and reboot: the 20 s hold plus triple click. */
void mirror_pair_factory_reset(void);

#ifdef __cplusplus
}
#endif
