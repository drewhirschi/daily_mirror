/*
 * The upload spool: every capture lands on flash before anything is attempted
 * over the network, and a background task drains it oldest-first.
 *
 * Until now a press uploaded inline, once, and a failure dropped the photo on
 * the floor - a Wi-Fi hiccup, a server deploy or a five-minute outage cost
 * someone their photo, and the press that took it had already flashed green.
 * That is the bug this module exists to close.
 *
 * Shape:
 *
 *   press  -> mirror_spool_add() writes <header><meta json><jpeg> to a temp
 *             name and renames it into the queue, then returns. The button is
 *             free again; nothing on the press path touches the network.
 *   drain  -> one task, one upload at a time, lowest sequence number first.
 *             Success deletes the file. Failure backs off 30 s -> 15 min with
 *             jitter, and the backoff resets the moment the station gets an
 *             address again, because the overwhelmingly common failure is the
 *             network having been away.
 *
 * Time. A capture that happens before SNTP has answered has no honest
 * timestamp, and the server is about to reject implausible ones, so such a
 * photo is held rather than uploaded with a 1970 date; it is stamped with the
 * time at which the clock first becomes valid. That can only happen in the
 * window between boot and the first SNTP reply, so it is seconds late at
 * worst, and the queue order is the capture order regardless.
 *
 * Failure classes, because "retry forever" is wrong for most of them:
 *
 *   transport / 5xx / 408 / 429   retry with backoff - the usual case
 *   401                           the token is dead. Stop draining entirely,
 *                                 keep every photo, say so in /stats. Retrying
 *                                 cannot help and deleting would be theft.
 *   400 / 409 / 413               this photo will never be accepted. Move it
 *                                 to rejected/ (at most 2 kept) so one bad
 *                                 file cannot wedge the queue behind it.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/** What caused the shutter, for the capture metadata. */
typedef enum {
    MIRROR_TRIGGER_BUTTON = 0,   /* the physical button */
    MIRROR_TRIGGER_DEBUG,        /* POST /press or POST /upload over the LAN */
} mirror_trigger_t;

const char *mirror_trigger_name(mirror_trigger_t t);

/**
 * Mount the spool partition, scan what is already queued, and start the
 * drain task. Call after the network is up.
 */
esp_err_t mirror_spool_start(void);

/**
 * Take ownership of a freshly captured JPEG: write it to the queue with its
 * metadata. Blocking on flash only - never on the network.
 *
 * The metadata is taken from the board's capture latch, read immediately
 * after the frame. When the queue has no room the OLDEST entry is dropped to make space, which
 * is counted; a full spool should lose the photo from last week, not the one
 * just taken.
 */
esp_err_t mirror_spool_add(const uint8_t *jpeg, size_t len, mirror_trigger_t trigger);

/** How many entries are waiting. */
uint32_t mirror_spool_count(void);

/** True while an upload is actually on the wire. */
bool mirror_spool_uploading(void);

/** Append the spool's `key=value` lines to a /stats buffer. Returns bytes written. */
int mirror_spool_stats(char *buf, size_t cap);

/** The metadata JSON of the most recent capture, for /stats. "" before the first. */
const char *mirror_spool_last_meta(void);

#ifdef __cplusplus
}
#endif
