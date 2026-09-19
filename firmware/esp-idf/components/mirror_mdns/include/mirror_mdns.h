/*
 * Daily Mirror LAN discovery: hostname mirror-<last 3 MAC bytes>.local and a
 * _dailymirror._tcp service whose TXT records let the app match a device on
 * the network to a device in the household (id), and spot unclaimed ones.
 * Contract between the shared app and this component.
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    const char *board;        /* e.g. "esp32p4-imx519", "esp32s3-ov5640" */
    const char *fw_version;   /* e.g. "0.1.0" */
    bool        claimed;      /* true once a household owns the device */
    uint16_t    http_port;    /* admin server port, normally 80 */
} mirror_mdns_info_t;

/** 12 lowercase hex chars of the station MAC plus NUL: the stable device id. */
void mirror_device_id(char out[13]);

/** Start mDNS. Call after the network interface has an address. */
esp_err_t mirror_mdns_start(const mirror_mdns_info_t *info);

/** Update the "claimed" TXT record in place. */
esp_err_t mirror_mdns_set_claimed(bool claimed);

/** "mirror-xxxxxx" (without ".local"); valid after mirror_mdns_start(). */
const char *mirror_mdns_hostname(void);

#ifdef __cplusplus
}
#endif
