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

/**
 * Get the camera ready to be grabbed from, and return when it is.
 *
 * Everything that takes time and is not the photograph: the autofocus pass,
 * and flushing the frames the pipeline exposed before it. It exists so the
 * press flow can do that work *underneath* the countdown blinks rather than
 * after them - the old flow ran a three-second focus-and-flush once the
 * countdown had finished, which is why the ring went on blinking (fast, and
 * apparently forever) after it had counted down to nothing.
 *
 * @param budget_ms  How long the caller is prepared to wait. Autofocus is
 *                   abandoned rather than allowed to overrun it; a photo
 *                   taken slightly soft at the promised moment is better than
 *                   a sharp one taken whenever the lens felt ready. The
 *                   capture info records what the focus actually did.
 *
 * The next board_camera_capture() then only grabs, so the shutter lands where
 * the countdown said it would. Safe to skip; capture still works without it.
 */
void board_camera_prepare_focus(uint32_t budget_ms);

/**
 * Settle auto-exposure and white balance on the scene as it is *now*.
 *
 * Split from the focus pass on purpose. The flash comes on between the two,
 * and the frames this discards are what lets AE meter the lit room rather
 * than the dark one - flush before the light and the photograph is blown out.
 * docs/burst-processing-experiment.md says the same thing from the other end:
 * hold the illumination steady through settling and the whole capture, never
 * strobe it.
 */
void board_camera_prepare_settle(uint32_t budget_ms);

/* ---- the capture flash ------------------------------------------------- */

/** True if this board has a flash pin configured. */
bool board_flash_available(void);

/**
 * Light or extinguish the capture flash.
 *
 * Turning it on arms a hard timeout inside the board layer. Nothing above
 * here is trusted to be the thing that switches it off - not an error path,
 * not an abort between the shutter and the "off".
 *
 * Deliberately on/off rather than a brightness: the pin is driven straight
 * today and there is nothing to dim. When the MOSFET stage arrives this grows
 * a board_flash_set_level() beside it and the LEDC channel lives in the board
 * adapter, where the LED PWM on the P4 already does; no caller above changes.
 */
void board_flash_set(bool on);

/** What the flash was doing for the last capture, for the metadata. */
bool board_flash_on(void);

/** Configure the pin and force the light off. Call early at start-up. */
esp_err_t board_flash_init(void);

/** Hand a board_camera_capture() buffer back. Safe with NULL. */
void board_camera_release(uint8_t *jpeg);

/** Dimensions of the last capture, for /stats and logs. Zero before the first. */
void board_camera_last_size(uint32_t *w, uint32_t *h);

/**
 * What the sensor was doing for the frame board_camera_capture() just handed
 * over: the exposure and gain it was actually using, how bright it thought the
 * scene was, and where autofocus ended up.
 *
 * This is the photo's provenance, and it only means anything if it is read
 * from the sensor immediately after that frame is grabbed - auto-exposure
 * moves between frames, so a read a second later describes a different photo.
 * Each adapter therefore latches these at grab time and this call just returns
 * the latch.
 *
 * Every field is optional. A board that cannot read one leaves it at the
 * "unknown" value below, and the upload omits it rather than guessing:
 *
 *   sensor        NULL or "" if the sensor is not identified
 *   width/height  0
 *   jpeg_quality  -1  (otherwise 0-100, 100 best, whatever was actually applied)
 *   exposure_us   -1
 *   analog_gain   -1  (otherwise a multiplier: 1.0 is unity gain)
 *   mean_luma     -1  (otherwise 0-255)
 *   af_state      "unknown" | "focused" | "failed" | "searching"
 *   focus_score   -1
 */
typedef struct {
    bool        valid;
    const char *sensor;
    uint32_t    width;
    uint32_t    height;
    int         jpeg_quality;
    int32_t     exposure_us;
    float       analog_gain;
    int         mean_luma;
    const char *af_state;
    int         focus_score;
    bool        flash;        /* the capture light was on for this frame */
} board_camera_capture_info_t;

/** The latch described above. Always fills `out`; never fails. */
void board_camera_capture_info(board_camera_capture_info_t *out);

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
