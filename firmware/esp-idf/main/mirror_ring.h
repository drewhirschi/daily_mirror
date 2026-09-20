/*
 * The ring: one logical RGB output, rendered on whatever LED the board has.
 *
 * The pattern names are the ones in crates/mirror-core/src/ring.rs, so the
 * Pi, the host adapter and this firmware all describe the device's state with
 * the same words.
 *
 * Both boards have a single LED rather than a ring of them, so the two
 * patterns that are spatial in the plan degrade to brightness over time: the
 * pairing "rotating chase" is an amber sawtooth, and the long-press "fill" is
 * an amber level that rises with the fraction held.
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
    RING_BLUE_SLOW_PULSE,    /* joining / claiming - see the note below */
    RING_GREEN_SLOW_PULSE,   /* uploading, or a backlog being worked through */
    RING_GREEN_TRIPLE_FLASH, /* the upload landed / the spool is empty */
    RING_WHITE_FLASH,        /* the shutter: the photo was taken just now */
    RING_RED_TRIPLE_PULSE,   /* error */
    RING_SOLID_RED,          /* hard failure, or the full reset is armed */
    RING_AMBER_CHASE,        /* pairing: discoverable, the app can connect */
    RING_AMBER_FAST_PULSE,   /* awaiting confirm: press the button to accept */
    RING_AMBER_FILL,         /* long-press in progress; see mirror_ring_fill() */
    RING_RED_FLASH,          /* one click of the reset confirmation */
} ring_pattern_t;

/*
 * Layers.
 *
 * The ring used to be one variable that whoever spoke last owned, which meant
 * every caller had to know what to restore when it finished - and got it
 * wrong as soon as two of them overlapped, because an upload finishing would
 * happily wipe the countdown of the press that interrupted it. Each concern
 * now writes its own layer and the topmost active one is rendered; when it
 * clears, whatever is underneath comes back by itself.
 *
 *   BASE     idle, and pairing/claiming/reset - mirror_ring_set()
 *   UPLOAD   the spool: green slow pulse while sending, green triple when the
 *            queue empties
 *   CAPTURE  a press: the countdown, the shutter flash, a capture error
 *
 * CAPTURE above UPLOAD is the rule Drew asked for in words: a button press
 * always wins immediately. The one inversion is pairing, which lives on BASE
 * and still outranks UPLOAD - see mirror_ring.c - because "press the button to
 * confirm" must not be hidden by a photo going out. It does not need to
 * outrank CAPTURE: mirror_pair_confirm() swallows the press before a capture
 * can start.
 */
typedef enum {
    RING_LAYER_BASE = 0,
    RING_LAYER_UPLOAD,
    RING_LAYER_CAPTURE,
    RING_LAYER_COUNT,
} ring_layer_t;

/** How long the press countdown runs, and how long the shutter cue shows. */
#define MIRROR_RING_COUNTDOWN_MS 4000
#define MIRROR_RING_SNAP_MS       160

/** Show `pattern` on `layer`. Returns immediately. */
void mirror_ring_layer_set(ring_layer_t layer, ring_pattern_t pattern);

/** Stop contributing to the ring from `layer`; what is underneath resumes. */
void mirror_ring_layer_clear(ring_layer_t layer);

/**
 * Show `pattern` on `layer` for its natural length, then clear the layer.
 * Blocking, so the caller's task is the one that waits.
 */
void mirror_ring_layer_play(ring_layer_t layer, ring_pattern_t pattern);

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

/**
 * Show RING_AMBER_FILL at `percent` of the way from the 2 s arm point to the
 * 5 s long-press. Called on every button poll while a hold is in progress, so
 * unlike mirror_ring_set() it does not restart the animation each time.
 */
void mirror_ring_fill(uint8_t percent);

#ifdef __cplusplus
}
#endif
