/*
 * Co-processor link watchdog (ESP32-P4 only; a no-op on the S3).
 *
 * The P4 has no radio. Every packet goes over SDIO to the ESP32-C6, and when
 * that link dies it does not always say so: with a mismatched esp_hosted host
 * and slave, the symptom was a device that kept running, kept answering
 * nothing, and silently stopped uploading. The one thing that reliably
 * recovers it is a restart, which also pulses the C6's reset line
 * (CONFIG_ESP_HOSTED_SLAVE_RESET_ON_EVERY_HOST_BOOTUP). The spool is on
 * flash, so the photos waiting in it survive the restart.
 *
 * This restarts the device, with the reason in the log, when:
 *
 *   - esp_hosted reports a transport failure;
 *   - the C6's heartbeat stops. The heartbeat is only switched on when the
 *     slave reports a real version (> 0.0.0). The factory firmware cannot
 *     report one, and is not trusted with a feature whose absence would read
 *     as a dead link;
 *   - photos are waiting and, for MIRROR_LINK_STALL_S, the station has been
 *     off the network or no upload has had any response from the server.
 *
 * Restarts for the last reason back off, doubling up to an hour, so a server
 * that is down for an evening costs a handful of reboots and not a hundred.
 */
#pragma once

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Check the co-processor's version, turn its heartbeat on if it can be
 * trusted, and start watching. Call after mirror_net_start() (the link has to
 * be up) and mirror_spool_start().
 */
esp_err_t mirror_link_start(void);

#ifdef __cplusplus
}
#endif
