/*
 * The press flow, and the "last photo" slot it commits into.
 *
 * One press:
 *   countdown (amber slow pulse, 3 x 650/350)
 *   -> focus and capture (amber rapid pulse)
 *   -> committed to the PSRAM slot behind /last.jpg (green flash, 700 ms)
 *   -> written to the upload spool on flash
 *   -> ready (soft white).
 *
 * The upload is NOT part of a press any more. It happens on the spool's drain
 * task, minutes or hours later if that is what the network requires, and the
 * ring shows it with the same blue slow pulse it always did. A press is over
 * when the photo is on flash, which is what makes the button feel instant and
 * what makes an outage cost nothing.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"
#include "mirror_spool.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Start the button task. Does nothing if the board has no button GPIO. */
esp_err_t mirror_press_start(void);

/** True while a press is being served. A second press is ignored, not queued. */
bool mirror_press_busy(void);

/**
 * Run the flow now, as POST /press does. Blocking; several seconds - the
 * capture and the flash write, but never the network.
 */
void mirror_press_run(mirror_trigger_t trigger);

/** How many presses have committed a photo since boot. */
uint32_t mirror_press_count(void);

/**
 * Borrow the committed JPEG. Returns false if there is none yet. On true the
 * caller MUST call mirror_press_last_release() - the press flow blocks on the
 * same lock, so holding it stalls the button.
 */
bool mirror_press_last_acquire(const uint8_t **jpeg, size_t *len);
void mirror_press_last_release(void);

#ifdef __cplusplus
}
#endif
