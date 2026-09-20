/*
 * The upload contract, identical to the Pi's (device/src/main.rs, fn upload):
 *
 *   1. POST {server_url}/api/uploads
 *        Authorization: Bearer <upload_token>
 *        {"capture_id": ..., "content_type": "image/jpeg", "content_length": N,
 *         "capture": { ...provenance... }}
 *      -> a grant {url, method, headers, complete_url}
 *   2. PUT (or POST, if the grant says so) the JPEG to the grant's url, with
 *      the grant's headers. The bearer token goes along ONLY when the target
 *      is the same origin as server_url - a signed URL on object storage must
 *      not be handed our credentials.
 *   3. POST complete_url.
 *
 * Step 2 streams from an open file in chunks. It is not an optimisation: a
 * 386 KB JPEG plus mbedTLS' own buffers is more than this firmware's internal
 * heap low-water mark can absorb, and the file is already on flash because it
 * came out of the spool.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    const char *capture_id;   /* `YYYYMMDDTHHMMSSZ-<8 hex>` */
    const char *meta_json;    /* the "capture" object, or NULL to omit it */
    FILE       *f;            /* positioned at the first JPEG byte */
    size_t      len;          /* JPEG length in bytes */
} mirror_upload_req_t;

/** True when both a server URL and a token are configured. */
bool mirror_upload_configured(void);

/**
 * Run the three-step upload, streaming the body from `req->f`. Blocking;
 * seconds.
 *
 * @param[out] http_status  The status code of whichever step ended it - 0 if
 *                          the failure was at the transport level. The caller
 *                          needs this to tell "retry later" (5xx, 408, 429)
 *                          from "this token is dead" (401) and "this photo
 *                          will never be accepted" (400/409/413).
 */
esp_err_t mirror_upload_file(const mirror_upload_req_t *req, int *http_status);

/** One line describing the last attempt, for the admin page and /stats. */
const char *mirror_upload_last_result(void);

/**
 * Failure injection, for testing the spool's retry path from the LAN without
 * touching the stored claim.
 *
 * While this is on, every upload fails at the transport level as though the
 * server were unroutable. It lives only in RAM, so a reboot clears it, and
 * nothing in the firmware ever turns it on by itself.
 */
void mirror_upload_set_blocked(bool blocked);
bool mirror_upload_blocked(void);

#ifdef __cplusplus
}
#endif
