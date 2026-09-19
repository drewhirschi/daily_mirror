/*
 * The upload contract, identical to the Pi's (device/src/main.rs, fn upload):
 *
 *   1. POST {server_url}/api/uploads
 *        Authorization: Bearer <upload_token>
 *        {"capture_id": ..., "content_type": "image/jpeg", "content_length": N}
 *      -> a grant {url, method, headers, complete_url}
 *   2. PUT (or POST, if the grant says so) the JPEG to the grant's url, with
 *      the grant's headers. The bearer token goes along ONLY when the target
 *      is the same origin as server_url - a signed URL on object storage must
 *      not be handed our credentials.
 *   3. POST complete_url.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/** True when both a server URL and an upload token are configured. */
bool mirror_upload_configured(void);

/** Run the three-step upload. Blocking; seconds. */
esp_err_t mirror_upload_jpeg(const uint8_t *jpeg, size_t len);

/** One line describing the last attempt, for the admin page and /stats. */
const char *mirror_upload_last_result(void);

#ifdef __cplusplus
}
#endif
