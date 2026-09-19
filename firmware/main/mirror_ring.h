/*
 * The ring: one logical RGB output, rendered on whatever LED the board has.
 *
 * The pattern names are the ones in crates/mirror-core/src/ring.rs, so the
 * Pi, the host adapter and this firmware all describe the device's state with
 * the same words. Only the subset the press flow actually uses is implemented;
 * the pairing patterns arrive with pairing.
 *
 * A renderer task owns the LED and animates whatever pattern is current, so
 * setting one never blocks. That matters for BLUE_SLOW_PULSE in particular:
 * it has to keep breathing through a multi-second upload that is running on
 * the caller's task.
 */
#pragma once

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    RING_OFF = 0,
    RING_SOLID_WHITE,        /* ready */
    RING_DIM_WHITE_BREATHE,  /* unprovisioned: works offline, not yet paired */
    RING_AMBER_SLOW_PULSE,   /* countdown: three slow pulses, 650 on / 350 off */
    RING_AMBER_RAPID_PULSE,  /* capturing: hold still */
    RING_GREEN_FLASH,        /* the JPEG is committed */
    RING_BLUE_SLOW_PULSE,    /* joining / claiming / uploading */
    RING_RED_TRIPLE_PULSE,   /* error */
    RING_SOLID_RED,          /* hard failure, nothing to retry */
} ring_pattern_t;

/** Start the renderer task. The board's LED must already be initialised. */
esp_err_t mirror_ring_start(void);

/** Show a pattern. Returns immediately; the renderer animates it. */
void mirror_ring_set(ring_pattern_t pattern);

/**
 * Show a pattern that has a natural length - the countdown, the green flash,
 * the red triple - and block for exactly that long, then leave the ring on
 * `next`. Timings match device/src/main.rs so the two feel identical.
 */
void mirror_ring_play(ring_pattern_t pattern, ring_pattern_t next);

#ifdef __cplusplus
}
#endif
