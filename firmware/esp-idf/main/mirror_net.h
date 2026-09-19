/*
 * Wi-Fi bring-up: station from the stored settings, with a SoftAP fallback so
 * a device that cannot join anything is still configurable.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"

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

/** True once the station holds an address. */
bool mirror_net_sta_connected(void);

/** True while the fallback access point is running. */
bool mirror_net_ap_active(void);

/** Dotted quad of whichever interface is up: the station's if it has one. */
const char *mirror_net_ip(void);

/** The access point's SSID (= the mDNS hostname), or "" if it is not up. */
const char *mirror_net_ap_ssid(void);

#ifdef __cplusplus
}
#endif
