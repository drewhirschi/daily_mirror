/*
 * Face-steered metering on the P4: a low-priority task finds the largest face
 * in a downscaled copy of the published frame and points the ISP's AE and AF
 * statistics windows at it. See face_ae_p4.c.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Fill `dst` (dw x dh RGB565) with every `step`th pixel of every `step`th row
 * of the newest frame. Called from the face task; takes the frame lock, so it
 * has to be quick.
 */
typedef bool (*face_ae_grab_fn)(uint16_t *dst, uint32_t dw, uint32_t dh, uint32_t step);

/**
 * Start the task.
 *
 * @param frame_w/h   The published frame, which is what grab() samples.
 * @param isp_w/h     The ISP's input - the sensor mode after binning - which is
 *                    the space the statistics windows are in. Equal to the
 *                    frame unless the ISP crops; a crop is assumed centred.
 */
esp_err_t face_ae_start(uint32_t frame_w, uint32_t frame_h, uint32_t isp_w, uint32_t isp_h,
                        face_ae_grab_fn grab);

/** True while a face is steering AE/AF, i.e. the windows are not the defaults. */
bool face_ae_steering(void);

/** Append face=, face_box=, face_age_ms=, detect_ms= ... lines for /stats. */
int face_ae_stats(char *out, size_t cap);

/**
 * Run the detector once on a JPEG and write what it found to `out`. With
 * `steer`, the largest face is also fed to the tracker as though the camera
 * had seen it, scaled from the JPEG's size to the frame's - a way to exercise
 * the AE/AF steering with nobody in front of the lens.
 */
esp_err_t face_ae_test_jpeg(const uint8_t *jpeg, size_t len, bool steer, char *out, size_t cap);

#ifdef __cplusplus
}
#endif
