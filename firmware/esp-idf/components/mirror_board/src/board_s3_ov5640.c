/*
 * Board adapter: Freenove ESP32-S3-WROOM CAM + OV5640 (autofocus module).
 *
 * DVP through esp32-camera, with the OV5640 producing JPEG itself, so there is
 * no encode step and no published-frame copy the way the P4 needs - the driver
 * hands over a frame buffer and takes it back. Autofocus is the OV5640's
 * on-chip MCU running a firmware blob that has to be uploaded at start-up.
 *
 * Status LED is the single WS2812 on the board, driven over RMT.
 */
#include <string.h>
#include <inttypes.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_camera.h"
#include "led_strip.h"
#include "driver/gpio.h"

#include "mirror_board.h"
#include "ov5640_af_cfg.h"

static const char *TAG = "board_s3";

static led_strip_handle_t s_led;
static bool s_af_ready;
static uint32_t s_w, s_h;
static camera_fb_t *s_held;            /* the frame board_camera_release() owes back */
static char s_status[72] = "camera not started";

const char *board_name(void) { return "ESP32-S3 + OV5640"; }
const char *board_id(void)   { return "esp32s3-ov5640"; }

/* ---- OV5640 autofocus firmware ----------------------------------------
 * The blob in ov5640_af_cfg.h is loaded into the sensor's MCU at 0x8000 while
 * the MCU is held in reset (0x3000 bit 5), then released. It reports
 * S_IDLE once it is alive, which takes a couple of seconds.
 *
 * vTaskDelay(1) - one tick - not pdMS_TO_TICKS(5): at the default 100 Hz that
 * rounds to *zero* ticks, and the poll loop below becomes a tight spin that
 * starves the very I2C traffic it is waiting on.
 */
static int ov5640_af_init(sensor_t *s)
{
    if (s->set_reg(s, 0x3000, 0xff, 0x20) < 0) {
        return -1;
    }
    uint16_t addr = 0x8000;
    for (size_t i = 0; i < sizeof(OV5640_AF_Config); i++, addr++) {
        if (s->set_reg(s, addr, 0xff, OV5640_AF_Config[i]) < 0) {
            return -1;
        }
    }
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, 0x00);
    s->set_reg(s, OV5640_CMD_ACK, 0xff, 0x00);
    for (uint16_t r = OV5640_CMD_PARA0; r <= OV5640_CMD_PARA4; r++) {
        s->set_reg(s, r, 0xff, 0x00);
    }
    s->set_reg(s, OV5640_CMD_FW_STATUS, 0xff, 0x7f);
    s->set_reg(s, 0x3000, 0xff, 0x00);
    for (int i = 0; i < 500; i++) {
        if (s->get_reg(s, OV5640_CMD_FW_STATUS, 0xff) == FW_STATUS_S_IDLE) {
            return 0;
        }
        vTaskDelay(1);
    }
    return 1;
}

/* One focus pass. Returns the firmware status byte, or -1. */
static int ov5640_af_single(sensor_t *s)
{
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, 0x01);
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, 0x08);
    for (int i = 0; i < 100 && s->get_reg(s, OV5640_CMD_ACK, 0xff) != 0; i++) {
        vTaskDelay(1);
    }
    s->set_reg(s, OV5640_CMD_ACK, 0xff, 0x01);
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, AF_TRIG_SINGLE_AUTO_FOCUS);
    for (int i = 0; i < 300; i++) {
        if (s->get_reg(s, OV5640_CMD_ACK, 0xff) == 0) {
            break;
        }
        vTaskDelay(1);
    }
    int st = -1;
    for (int i = 0; i < 300; i++) {
        st = s->get_reg(s, OV5640_CMD_FW_STATUS, 0xff);
        if (st == FW_STATUS_S_FOCUSED) {
            break;
        }
        vTaskDelay(1);
    }
    return st;
}

esp_err_t board_camera_init(void)
{
    /*
     * esp32-camera's quality scale is inverted and 0-63, where 0 is best;
     * the shared MIRROR_JPEG_QUALITY knob is 0-100 with 100 best, as the P4's
     * hardware encoder uses. Map one onto the other here so the two boards
     * answer the same setting.
     */
    int q = 63 - (CONFIG_MIRROR_JPEG_QUALITY * 63) / 100;
    if (q < 0) {
        q = 0;
    }

    camera_config_t config = {
        .pin_pwdn = -1, .pin_reset = -1,
        .pin_xclk = CONFIG_MIRROR_CAM_XCLK_GPIO,
        .pin_sccb_sda = CONFIG_MIRROR_CAM_SDA_GPIO,
        .pin_sccb_scl = CONFIG_MIRROR_CAM_SCL_GPIO,
        .pin_d7 = CONFIG_MIRROR_CAM_D7_GPIO, .pin_d6 = CONFIG_MIRROR_CAM_D6_GPIO,
        .pin_d5 = CONFIG_MIRROR_CAM_D5_GPIO, .pin_d4 = CONFIG_MIRROR_CAM_D4_GPIO,
        .pin_d3 = CONFIG_MIRROR_CAM_D3_GPIO, .pin_d2 = CONFIG_MIRROR_CAM_D2_GPIO,
        .pin_d1 = CONFIG_MIRROR_CAM_D1_GPIO, .pin_d0 = CONFIG_MIRROR_CAM_D0_GPIO,
        .pin_vsync = CONFIG_MIRROR_CAM_VSYNC_GPIO,
        .pin_href = CONFIG_MIRROR_CAM_HREF_GPIO,
        .pin_pclk = CONFIG_MIRROR_CAM_PCLK_GPIO,
        .xclk_freq_hz = 20000000,
        .ledc_timer = LEDC_TIMER_0, .ledc_channel = LEDC_CHANNEL_0,
        .pixel_format = PIXFORMAT_JPEG,
        /* Buffers are sized for the largest frame this sensor can produce, so
         * asking for 5 MP here and dropping to 2 MP below means a later
         * switch up to 5 MP needs no reallocation. */
        .frame_size = FRAMESIZE_QSXGA,
        .jpeg_quality = q,
        .fb_count = 2, .fb_location = CAMERA_FB_IN_PSRAM,
        .grab_mode = CAMERA_GRAB_LATEST,
    };
    esp_err_t err = esp_camera_init(&config);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "esp_camera_init failed: %s", esp_err_to_name(err));
        return err;
    }

    sensor_t *s = esp_camera_sensor_get();
    ESP_LOGI(TAG, "sensor PID=0x%04x", s->id.PID);
    s->set_framesize(s, FRAMESIZE_UXGA);
    /* A Kconfig bool that is 'n' is not defined at all, so these have to be
     * #ifdef, not a ternary on the macro. */
#ifdef CONFIG_MIRROR_CAM_VFLIP
    s->set_vflip(s, 1);
#else
    s->set_vflip(s, 0);
#endif
#ifdef CONFIG_MIRROR_CAM_HMIRROR
    s->set_hmirror(s, 1);
#else
    s->set_hmirror(s, 0);
#endif
    s->set_ae_level(s, -2);
    s_w = 1600;
    s_h = 1200;

    if (s->id.PID == OV5640_PID) {
        s_af_ready = ov5640_af_init(s) == 0;
        ESP_LOGI(TAG, "AF firmware %s", s_af_ready ? "ready" : "failed");
    }
    snprintf(s_status, sizeof(s_status), "OV5640 PID 0x%04x %" PRIu32 "x%" PRIu32 ", AF %s",
             s->id.PID, s_w, s_h, s_af_ready ? "single-shot" : "unavailable");
    return ESP_OK;
}

esp_err_t board_camera_capture(bool autofocus, uint8_t **jpeg, size_t *len)
{
    *jpeg = NULL;
    *len = 0;
    if (s_held) {
        /* A caller that forgot to release. Do not leak the pipeline dry. */
        esp_camera_fb_return(s_held);
        s_held = NULL;
    }

    sensor_t *s = esp_camera_sensor_get();
    if (!s) {
        return ESP_ERR_INVALID_STATE;
    }
    if (autofocus && s_af_ready) {
        int st = ov5640_af_single(s);
        ESP_LOGI(TAG, "AF status 0x%02x", st);
    }

    /*
     * Drain before taking the keeper. With CAMERA_GRAB_LATEST the driver still
     * has frames in flight that were exposed before the focus pass, and those
     * are the blurry ones.
     */
    for (int i = 0; i < 6; i++) {
        camera_fb_t *f = esp_camera_fb_get();
        if (f) {
            esp_camera_fb_return(f);
        }
    }

    camera_fb_t *fb = esp_camera_fb_get();
    if (!fb) {
        return ESP_FAIL;
    }
    s_held = fb;
    s_w = fb->width;
    s_h = fb->height;
    *jpeg = fb->buf;
    *len = fb->len;
    return ESP_OK;
}

void board_camera_release(uint8_t *jpeg)
{
    (void)jpeg;
    if (s_held) {
        esp_camera_fb_return(s_held);
        s_held = NULL;
    }
}

void board_camera_last_size(uint32_t *w, uint32_t *h)
{
    if (w) *w = s_w;
    if (h) *h = s_h;
}

const char *board_camera_status(void) { return s_status; }

esp_err_t board_led_init(void)
{
    led_strip_config_t cfg = {
        .strip_gpio_num = CONFIG_MIRROR_LED_WS2812_GPIO, .max_leds = 1,
        .led_model = LED_MODEL_WS2812,
        .color_component_format = LED_STRIP_COLOR_COMPONENT_FMT_GRB,
    };
    led_strip_rmt_config_t rmt = {
        .clk_src = RMT_CLK_SRC_DEFAULT, .resolution_hz = 10 * 1000 * 1000,
    };
    if (led_strip_new_rmt_device(&cfg, &rmt, &s_led) != ESP_OK) {
        s_led = NULL;
        ESP_LOGW(TAG, "no WS2812 on GPIO %d - running without a status LED",
                 CONFIG_MIRROR_LED_WS2812_GPIO);
        return ESP_FAIL;
    }
    return ESP_OK;
}

void board_led_set_rgb(uint8_t r, uint8_t g, uint8_t b)
{
    if (s_led) {
        led_strip_set_pixel(s_led, 0, r, g, b);
        led_strip_refresh(s_led);
    }
}

int board_button_gpio(void) { return CONFIG_MIRROR_BUTTON_GPIO; }

esp_err_t board_net_init(void)
{
    /* The S3 has its own radio; nothing to arrange. */
    return ESP_OK;
}
