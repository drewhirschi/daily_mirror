/*
 * Wi-Fi bring-up: station from the stored settings, with a SoftAP fallback so
 * a device that cannot join anything is still configurable.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"
#include "esp_netif.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Start networking and return as soon as *something* is reachable - a station
 * IP, or the fallback access point - so the admin server can be started.
 *
 * With an SSID stored, the station keeps trying forever in the background
 * (1 s between the first few attempts, then 10 s), because an appliance that
 * drops off Wi-Fi overnight has to come back on its own. The access point
 * comes up alongside it if no address arrives within
 * CONFIG_MIRROR_STA_TIMEOUT_S, and the station keeps trying underneath it.
 *
 * With no SSID stored, the access point comes up immediately.
 */
esp_err_t mirror_net_start(void);

/**
 * Put the radio back into APSTA if the fallback access point is up.
 *
 * The provisioning manager forces WIFI_MODE_STA when it starts, because BLE
 * provisioning has no use for an access point - which would silently take the
 * bench setup network, and /config with it, off the air for the whole pairing
 * window. Call this straight after starting the pairing service.
 */
void mirror_net_restore_ap_mode(void);

/**
 * Store the credentials the station is currently using and start the
 * keep-trying reconnect loop. Called once a claim succeeds, so the Wi-Fi the
 * app handed over through provisioning survives the next boot.
 */
esp_err_t mirror_net_adopt_sta_credentials(void);

/** True once the station holds an address. */
bool mirror_net_sta_connected(void);

/**
 * True once SNTP has set a plausible wall clock.
 *
 * Capture IDs are UTC timestamps and the server derives photos.captured_at
 * from them, so an upload sent before this is true files the photo under 1970.
 */
bool mirror_net_clock_valid(void);

/** True while the fallback access point is running. */
bool mirror_net_ap_active(void);

/** Dotted quad of whichever interface is up: the station's if it has one. */
const char *mirror_net_ip(void);

/** The access point's SSID (= the mDNS hostname), or "" if it is not up. */
const char *mirror_net_ap_ssid(void);

#ifdef __cplusplus
}
#endif
