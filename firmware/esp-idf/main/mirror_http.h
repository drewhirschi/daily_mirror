/*
 * The admin server on port 80:
 *
 *   GET  /               status page, named after the board
 *   GET  /snapshot.jpg   focus, capture, send - the live scene
 *   GET  /last.jpg       the last committed press photo
 *   POST /press          run the press flow
 *   POST /upload         capture and upload in one step
 *   GET  /stats          plain text: heap, camera, network, the upload spool
 *   GET  /logs           plain text: the last ~8 KB of the console log
 *   POST /debug/upload-block?on=1  make every upload fail, for testing retry
 *   GET  /config         from mirror_config, registered on this same server
 *
 * The same server serves the fallback access point, so /config is reachable on
 * a device that has never joined a network.
 */
#pragma once

#include "esp_err.h"
#include "esp_http_server.h"

#ifdef __cplusplus
extern "C" {
#endif

esp_err_t mirror_http_start(httpd_handle_t *out);

#ifdef __cplusplus
}
#endif
