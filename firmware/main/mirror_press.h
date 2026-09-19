/*
 * The press flow, and the "last photo" slot it commits into.
 *
 * One press:
 *   countdown (amber slow pulse, 3 x 650/350)
 *   -> focus and capture (amber rapid pulse)
 *   -> committed to the PSRAM slot behind /last.jpg (green flash, 700 ms)
 *   -> upload, if a server and token are configured (blue slow pulse)
 *   -> ready (soft white).
 * Anything that fails shows the red triple pulse and ends at ready anyway -
 * the photo is already safe in PSRAM by then, so a failed upload is not a
 * failed press.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Start the button task. Does nothing if the board has no button GPIO. */
esp_err_t mirror_press_start(void);

/** True while a press is being served. A second press is ignored, not queued. */
bool mirror_press_busy(void);

/** Run the flow now, as POST /press does. Blocking; several seconds. */
void mirror_press_run(void);

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
