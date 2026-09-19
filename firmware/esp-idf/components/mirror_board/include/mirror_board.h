/*
 * The board adapter: everything the shared app needs from the hardware, and
 * nothing it needs to know about how that hardware works.
 *
 * Exactly one src/board_*.c is compiled into a build, chosen by the
 * CONFIG_MIRROR_BOARD_* Kconfig choice (see firmware/README.md). The app in
 * firmware/main never includes esp_video, esp_camera, led_strip or ledc.
 *
 * Today the two implementations are as different as two camera stacks get:
 *
 *   p4_imx519   MIPI-CSI through esp_video, a continuously running capture
 *               task publishing RGB565 frames, the hardware JPEG encoder, an
 *               IMX519 with an AK7375 focus motor driven by the ISP's
 *               closed-loop AF, an RGB LED on three LEDC PWM channels, and
 *               Wi-Fi living on a separate ESP32-C6 over esp_hosted.
 *   s3_ov5640   DVP through esp32-camera, JPEG straight out of the sensor,
 *               an OV5640 running the community AF firmware blob, and a
 *               single WS2812 on RMT.
 *
 * Which is the point: if a call below can be answered the same way by both,
 * it belongs here; if it cannot, it does not belong in the app.
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Human-readable, for the admin page title and logs: "ESP32-P4 + IMX519". */
const char *board_name(void);

/** Machine-readable, for the mDNS TXT record: "esp32p4-imx519". */
const char *board_id(void);

/**
 * Bring the camera up and leave it ready to capture. On the P4 this also
 * starts the streaming capture task, so auto-exposure, white balance and
 * autofocus are converged by the time the first press arrives.
 */
esp_err_t board_camera_init(void);

/**
 * A JPEG of the scene now.
 *
 * @param autofocus  Run a focus pass first. On the S3 that is a blocking
 *                   single-shot AF (about a second); on the P4 the ISP's AF
 *                   is continuous, so this only allows it time to settle.
 * @param[out] jpeg  The encoded bytes. Valid until board_camera_release().
 * @param[out] len   Their length.
 *
 * The buffer belongs to the board layer - it may be a driver frame buffer that
 * the pipeline needs back - so the caller must always pair this with
 * board_camera_release(), and must copy anything it intends to keep.
 */
esp_err_t board_camera_capture(bool autofocus, uint8_t **jpeg, size_t *len);

/** Hand a board_camera_capture() buffer back. Safe with NULL. */
void board_camera_release(uint8_t *jpeg);

/** Dimensions of the last capture, for /stats and logs. Zero before the first. */
void board_camera_last_size(uint32_t *w, uint32_t *h);

/** One line about the camera for /stats, e.g. "IMX519 1920x1080, AF continuous". */
const char *board_camera_status(void);

/** The status LED. board_led_set_rgb() takes 0..255 per channel, 0,0,0 = off. */
esp_err_t board_led_init(void);
void board_led_set_rgb(uint8_t r, uint8_t g, uint8_t b);

/** GPIO of the momentary button: active low, wired to GND, internal pull-up. */
int board_button_gpio(void);

/**
 * Anything that has to happen before esp_wifi_init(), or ESP_OK if nothing
 * does. The P4 has no radio of its own - esp_wifi_remote forwards the calls
 * over SDIO to an ESP32-C6 running esp_hosted - and this is where that link
 * gets checked and reported as a radio problem rather than a network one.
 */
esp_err_t board_net_init(void);

#ifdef __cplusplus
}
#endif
