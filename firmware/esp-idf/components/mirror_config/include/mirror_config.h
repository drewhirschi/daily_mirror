/*
 * Daily Mirror device settings: NVS-backed key/value store plus the admin-page
 * form that edits it. This header is the contract between the shared app
 * (firmware/main) and this component; change it only with both sides in hand.
 */
#pragma once

#include <stdbool.h>
#include "esp_err.h"
#include "esp_http_server.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Setting keys. Values are UTF-8 strings, at most MIRROR_CONFIG_VALUE_MAX bytes. */
#define MIRROR_CFG_WIFI_SSID     "wifi_ssid"
#define MIRROR_CFG_WIFI_PASS     "wifi_pass"
#define MIRROR_CFG_SERVER_URL    "server_url"
#define MIRROR_CFG_UPLOAD_TOKEN  "upload_token"
#define MIRROR_CFG_DEVICE_NAME   "device_name"
/* Per-device bearer token minted by POST /api/devices/claim. Its presence is
 * what "claimed" means: with one stored the device boots Ready, without one it
 * boots into pairing. Written by mirror_pair, never by the /config form. */
#define MIRROR_CFG_DEVICE_TOKEN  "device_token"
/* Household the claim bound this device to; informational, for /stats. */
#define MIRROR_CFG_HOUSEHOLD_ID  "household_id"
/* One-shot: "1" means the next boot enters pairing even if a device token is
 * stored. Set by the long-press, because the Bluetooth stack pairing needs is
 * released once pairing ends and only a restart brings it back. */
#define MIRROR_CFG_PAIR_ON_BOOT  "pair_on_boot"
#define MIRROR_CONFIG_VALUE_MAX  256

/** Initialise NVS (erasing and retrying on a version mismatch) and load settings. Call once, first. */
esp_err_t mirror_config_init(void);

/**
 * Current value for a key, or "" if unset. Falls back to the build-time
 * default (Kconfig MIRROR_DEFAULT_*) when NVS has nothing. The pointer stays
 * valid until the next mirror_config_set() for that key.
 */
const char *mirror_config_get(const char *key);

/** Store a value (empty string clears it) and commit. */
esp_err_t mirror_config_set(const char *key, const char *value);

/** True when a Wi-Fi SSID is available (from NVS or a build-time default). */
bool mirror_config_has_wifi(void);

/** Erase every setting. */
esp_err_t mirror_config_erase_all(void);

/**
 * Register the settings routes on an already-started server:
 *   GET  /config         HTML form; secrets are shown masked, never echoed back
 *   POST /config         application/x-www-form-urlencoded; blank secret fields
 *                        mean "leave unchanged"; responds, then reboots
 *   POST /config/reset   erase all settings, respond, reboot
 */
esp_err_t mirror_config_register_http(httpd_handle_t server);

#ifdef __cplusplus
}
#endif
