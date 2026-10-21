/*
 * A C face for ESP-DL's human_face_detect, which is C++.
 *
 * Only face_ae_p4.c calls this, and only from its own task: the detector keeps
 * per-instance scratch state and is not safe to run from two tasks at once.
 */
#pragma once

#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    int x, y, w, h;   /* pixels of the image that was searched */
    float score;      /* 0-1, the second stage's confidence */
} face_box_t;

/** Load the model. Slow (a few hundred ms) and once; call from the face task. */
esp_err_t face_detect_init(void);

/**
 * Find faces in a little-endian RGB565 image (the ISP's output format). The
 * model resizes internally, so any size works, but it only ever looks at
 * 160x120: anything much bigger than ~320 wide is wasted reads.
 *
 * @return the number written to `out`, at most `max`, best score first.
 */
int face_detect_rgb565(const uint16_t *px, int w, int h, face_box_t *out, int max);

#ifdef __cplusplus
}
#endif
