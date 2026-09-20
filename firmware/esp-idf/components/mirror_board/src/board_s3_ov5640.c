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
#include "esp_timer.h"
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
static int s_quality_0_100 = -1;       /* what was actually applied, 100 = best */
static float s_line_time_us;           /* one sensor row; see measure_line_time() */
static bool s_prepared;                /* board_camera_prepare() has focused and flushed */
static bool s_prep_af_ran;
static int  s_prep_af_status = -1;
static board_camera_capture_info_t s_info = {
    .jpeg_quality = -1, .exposure_us = -1, .analog_gain = -1.0f,
    .mean_luma = -1, .af_state = "unknown", .focus_score = -1,
};

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

/*
 * One focus pass, abandoned if it overruns `deadline_us` on the esp_timer
 * clock. Returns the firmware status byte, or -1.
 *
 * The bound matters: this runs underneath the press countdown now, and the
 * shutter fires when the countdown ends whether the lens agrees or not. A
 * pass that is still hunting at the deadline reports whatever it has reached,
 * which latch_capture_info() turns into af_state "searching" - an honest
 * record that the photo may be soft, rather than a delay nobody asked for.
 */
static int ov5640_af_single_until(sensor_t *s, int64_t deadline_us)
{
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, 0x01);
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, 0x08);
    for (int i = 0; i < 100 && s->get_reg(s, OV5640_CMD_ACK, 0xff) != 0; i++) {
        if (esp_timer_get_time() > deadline_us) {
            return -1;
        }
        vTaskDelay(1);
    }
    s->set_reg(s, OV5640_CMD_ACK, 0xff, 0x01);
    s->set_reg(s, OV5640_CMD_MAIN, 0xff, AF_TRIG_SINGLE_AUTO_FOCUS);
    for (int i = 0; i < 300; i++) {
        if (s->get_reg(s, OV5640_CMD_ACK, 0xff) == 0) {
            break;
        }
        if (esp_timer_get_time() > deadline_us) {
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
        if (esp_timer_get_time() > deadline_us) {
            break;
        }
        vTaskDelay(1);
    }
    return st;
}

static int ov5640_af_single(sensor_t *s)
{
    return ov5640_af_single_until(s, esp_timer_get_time() + 6000000);
}

/* ---- capture provenance ------------------------------------------------
 *
 * The OV5640's exposure is expressed in sixteenths of a row, so turning it
 * into microseconds needs the row time - and that is where the OV5640's clock
 * tree gets treacherous. HTS (0x380C/0x380D) counts pixel periods per row, but
 * "pixel period" here is the sensor array's internal clock, which is not the
 * PCLK coming out of the DVP bus (esp32-camera divides that down again with
 * 0x3824/0x460C), and the published PLL formula in the driver's calc_sysclk()
 * disagrees with the datasheet's by a factor that depends on the bit-depth
 * register. Deriving it from the registers alone is how you get an exposure
 * figure that is confidently wrong.
 *
 * So it is measured instead, once, at start-up: time N frames, divide by N,
 * divide by VTS (rows per frame). Frame period is something the driver can
 * observe directly and it is the same number the sensor is counting rows
 * against, so this is ground truth rather than a reconstruction.
 *
 * The register-derived value is computed alongside it and logged, purely so
 * the two can be compared in the field; the measurement is what is used. See
 * the numbers in firmware/esp-idf/README.md.
 */
static int reg8(sensor_t *s, uint16_t reg)
{
    return s->get_reg(s, reg, 0xff);
}

static int reg16(sensor_t *s, uint16_t hi, int hi_mask)
{
    int h = s->get_reg(s, hi, 0xff);
    int l = s->get_reg(s, hi + 1, 0xff);
    if (h < 0 || l < 0) {
        return -1;
    }
    return ((h & hi_mask) << 8) | l;
}

/* calc_sysclk() from esp32-camera's ov5640.c, run backwards off the live PLL
 * registers. Only for the log line above. */
static float derived_line_time_us(sensor_t *s, int hts)
{
    static const float pre_div2x[] = { 1, 1, 2, 3, 4, 1.5f, 6, 2.5f, 8 };
    static const int   root_div[]  = { 1, 2, 4, 8 };

    int r3034 = reg8(s, 0x3034);
    int r3035 = reg8(s, 0x3035);
    int r3036 = reg8(s, 0x3036);
    int r3037 = reg8(s, 0x3037);
    int r3108 = reg8(s, 0x3108);
    int r3824 = reg8(s, 0x3824);
    int r460c = reg8(s, 0x460C);
    if (r3034 < 0 || r3035 < 0 || r3036 < 0 || r3037 < 0 || r3108 < 0 || hts <= 0) {
        return 0.0f;
    }

    int sys_div = (r3035 >> 4) & 0x0f;
    if (!sys_div) {
        sys_div = 1;
    }
    int mult = r3036 & 0xff;
    int pre = r3037 & 0x0f;
    if (pre > 8) {
        pre = 8;
    }
    bool root_2x = (r3037 & 0x10) != 0;
    int pclk_root = (r3108 >> 4) & 0x03;
    bool pclk_manual = (r460c & 0x02) != 0;
    int pclk_div = r3824 & 0x1f;

    float refin = 20000000.0f / pre_div2x[pre];
    float vco = refin * mult / (root_2x ? 2 : 1);
    /* 0x3034 == 0x1A is 10-bit mode: /5 * 2. 8-bit would be /4 * 2. */
    float pll_clk = vco / sys_div * 2.0f / ((r3034 & 0x0f) == 0x0a ? 5.0f : 4.0f);
    float pclk = pll_clk / root_div[pclk_root] / ((pclk_manual && pclk_div) ? pclk_div : 2);
    if (pclk <= 0) {
        return 0.0f;
    }
    return (float)hts * 1e6f / pclk;
}

static void measure_line_time(sensor_t *s)
{
    int hts = reg16(s, 0x380C, 0x0f);
    int vts = reg16(s, 0x380E, 0xff);
    float derived = derived_line_time_us(s, hts);

    /* Warm up: the first frames after a mode change are not representative. */
    for (int i = 0; i < 3; i++) {
        camera_fb_t *f = esp_camera_fb_get();
        if (f) {
            esp_camera_fb_return(f);
        }
    }
    const int n = 6;
    int64_t t0 = esp_timer_get_time();
    int got = 0;
    for (int i = 0; i < n; i++) {
        camera_fb_t *f = esp_camera_fb_get();
        if (!f) {
            break;
        }
        esp_camera_fb_return(f);
        got++;
    }
    int64_t elapsed = esp_timer_get_time() - t0;

    float measured = 0.0f;
    if (got > 0 && vts > 0 && elapsed > 0) {
        measured = (float)elapsed / got / vts;
    }

    /* A row is tens of microseconds at these clocks. Anything outside a very
     * generous band means the measurement was disturbed (a frame drop, or the
     * driver buffering) and the derivation is the better guess. */
    if (measured > 1.0f && measured < 2000.0f) {
        s_line_time_us = measured;
    } else {
        s_line_time_us = derived;
    }
    ESP_LOGI(TAG, "line time: measured %.2f us, reg-derived %.2f us "
                  "(HTS %d, VTS %d, %d frames in %lld us) - using %.2f",
             measured, derived, hts, vts, got, (long long)elapsed, s_line_time_us);
}

/* Latch what the sensor was doing for the frame just grabbed. */
static void latch_capture_info(sensor_t *s, int af_status, bool af_ran)
{
    s_info.valid = true;
    s_info.sensor = "ov5640";
    s_info.width = s_w;
    s_info.height = s_h;
    s_info.jpeg_quality = s_quality_0_100;

    /* 0x3500[3:0]:0x3501:0x3502 is a 20-bit exposure in 1/16 of a row. */
    int e2 = reg8(s, 0x3500), e1 = reg8(s, 0x3501), e0 = reg8(s, 0x3502);
    if (e2 >= 0 && e1 >= 0 && e0 >= 0 && s_line_time_us > 0.0f) {
        uint32_t sixteenths = ((uint32_t)(e2 & 0x0f) << 16) | ((uint32_t)e1 << 8) | (uint32_t)e0;
        float us = (float)sixteenths / 16.0f * s_line_time_us;
        s_info.exposure_us = (int32_t)(us + 0.5f);
    } else {
        s_info.exposure_us = -1;
    }

    /* 0x350A[1:0]:0x350B is real gain x16. */
    int gain = reg16(s, 0x350A, 0x03);
    s_info.analog_gain = gain >= 0 ? (float)gain / 16.0f : -1.0f;

    /* 0x56A1 is the AVG block's readout: the mean luma of the last frame. */
    int luma = reg8(s, 0x56A1);
    s_info.mean_luma = luma >= 0 ? luma : -1;

    if (!af_ran || af_status < 0) {
        s_info.af_state = "unknown";
    } else if (af_status == FW_STATUS_S_FOCUSED) {
        s_info.af_state = "focused";
    } else if ((af_status & 0x7f) <= 0x0f) {
        s_info.af_state = "searching";
    } else if (af_status == FW_STATUS_S_IDLE) {
        /* The single-shot pass ran and came back to idle without reaching
         * S_FOCUSED - it gave up. */
        s_info.af_state = "failed";
    } else {
        s_info.af_state = "unknown";
    }

    /* The community AF firmware blob publishes no sharpness metric - PARA0..4
     * carry zone configuration, not a score - so there is nothing honest to
     * report here. */
    s_info.focus_score = -1;
    s_info.flash = board_flash_on();
}

void board_camera_capture_info(board_camera_capture_info_t *out)
{
    if (out) {
        *out = s_info;
    }
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
    /* Back the other way, so the metadata reports the quality the sensor was
     * actually given rather than the one that was asked for - the round trip
     * through a 64-step inverted scale does not come back unchanged. */
    s_quality_0_100 = ((63 - q) * 100 + 31) / 63;

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

    /* Before the AF blob: this wants an undisturbed stream, and it is the
     * calibration every exposure figure downstream depends on. */
    measure_line_time(s);

    if (s->id.PID == OV5640_PID) {
        s_af_ready = ov5640_af_init(s) == 0;
        ESP_LOGI(TAG, "AF firmware %s", s_af_ready ? "ready" : "failed");
    }
    snprintf(s_status, sizeof(s_status), "OV5640 PID 0x%04x %" PRIu32 "x%" PRIu32 ", AF %s",
             s->id.PID, s_w, s_h, s_af_ready ? "single-shot" : "unavailable");
    return ESP_OK;
}

/* Discard whatever the pipeline exposed before the focus pass; with
 * CAMERA_GRAB_LATEST those frames are still in flight, and they are the
 * blurry ones. */
static void flush_frames(int n, int64_t deadline_us)
{
    for (int i = 0; i < n; i++) {
        if (esp_timer_get_time() > deadline_us) {
            return;
        }
        camera_fb_t *f = esp_camera_fb_get();
        if (f) {
            esp_camera_fb_return(f);
        }
    }
}

void board_camera_prepare_focus(uint32_t budget_ms)
{
    sensor_t *s = esp_camera_sensor_get();
    if (!s || !s_af_ready) {
        return;
    }
    int64_t deadline = esp_timer_get_time() + (int64_t)budget_ms * 1000;
    s_prep_af_status = ov5640_af_single_until(s, deadline);
    s_prep_af_ran = true;
    ESP_LOGI(TAG, "AF status 0x%02x (during the countdown)", s_prep_af_status);
}

void board_camera_prepare_settle(uint32_t budget_ms)
{
    /* Called after the flash is on, so these are the frames that teach
     * auto-exposure what the lit scene looks like. Flushing before the light
     * would settle AE on the dark room and blow the photograph out. */
    flush_frames(6, esp_timer_get_time() + (int64_t)budget_ms * 1000);
    s_prepared = true;
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
    int af_status = -1;
    bool af_ran = false;

    if (s_prepared) {
        /* board_camera_prepare() already focused and flushed, under the
         * countdown. Grab straight away - this is the shutter, and anything
         * done here is time between the last blink and the photo. */
        af_status = s_prep_af_status;
        af_ran = s_prep_af_ran;
        s_prepared = false;
        s_prep_af_ran = false;
        s_prep_af_status = -1;
    } else {
        af_ran = autofocus && s_af_ready;
        if (af_ran) {
            af_status = ov5640_af_single(s);
            ESP_LOGI(TAG, "AF status 0x%02x", af_status);
        }
        flush_frames(6, esp_timer_get_time() + 5000000);
    }

    camera_fb_t *fb = esp_camera_fb_get();
    if (!fb) {
        return ESP_FAIL;
    }
    s_held = fb;
    s_w = fb->width;
    s_h = fb->height;
    /* Immediately, before anything else touches the sensor: auto-exposure
     * moves between frames, and these registers have to describe THIS one. */
    latch_capture_info(s, af_status, af_ran);
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


/* ---- the capture flash -------------------------------------------------
 *
 * A light that comes on before the shutter and goes off straight after it.
 *
 * Two rules, both from docs/burst-processing-experiment.md and
 * docs/imx519-sensor-profile-guide.md: hold it steady through settling and the
 * whole capture rather than strobing it (a pulse shorter than a frame lights
 * only part of a rolling-shutter readout), and let auto-exposure meter the lit
 * scene, not the dark one.
 *
 * And one rule from the hardware: it must never be left on. The pin drives an
 * LED directly for now, with a MOSFET stage to come, and a light stuck on is a
 * hot part, a flat battery and a lamp in someone's living room all night. So
 * every turn-on arms a one-shot that forces it off, and nothing above the
 * board layer is trusted to be the thing that switches it off.
 */
#define FLASH_MAX_ON_MS 6000

static bool s_flash_on;
static esp_timer_handle_t s_flash_guard;

bool board_flash_available(void) { return CONFIG_MIRROR_FLASH_GPIO >= 0; }
bool board_flash_on(void)        { return s_flash_on; }

static void flash_drive(bool on)
{
#if CONFIG_MIRROR_FLASH_GPIO >= 0
#ifdef CONFIG_MIRROR_FLASH_ACTIVE_HIGH
    gpio_set_level(CONFIG_MIRROR_FLASH_GPIO, on ? 1 : 0);
#else
    gpio_set_level(CONFIG_MIRROR_FLASH_GPIO, on ? 0 : 1);
#endif
#else
    (void)on;
#endif
}

static void flash_guard_fired(void *arg)
{
    (void)arg;
    if (s_flash_on) {
        /* Something above did not switch it off. That is a bug wherever it
         * is, and it is not going to be paid for in heat. */
        ESP_LOGE(TAG, "flash was still on after %d ms - forcing it off",
                 FLASH_MAX_ON_MS);
        s_flash_on = false;
        flash_drive(false);
    }
}

esp_err_t board_flash_init(void)
{
#if CONFIG_MIRROR_FLASH_GPIO >= 0
    gpio_config_t io = {
        .pin_bit_mask = 1ULL << CONFIG_MIRROR_FLASH_GPIO,
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    esp_err_t err = gpio_config(&io);
    if (err != ESP_OK) {
        return err;
    }
    /* Off first, before anything can decide otherwise: a reset while the
     * light was on must not come back up with it still lit. */
    s_flash_on = false;
    flash_drive(false);

    const esp_timer_create_args_t args = {
        .callback = flash_guard_fired, .name = "flash_guard",
    };
    esp_timer_create(&args, &s_flash_guard);
    ESP_LOGI(TAG, "capture flash on GPIO %d (active %s)",
             CONFIG_MIRROR_FLASH_GPIO,
#ifdef CONFIG_MIRROR_FLASH_ACTIVE_HIGH
             "high"
#else
             "low"
#endif
             );
    return ESP_OK;
#else
    return ESP_OK;
#endif
}

void board_flash_set(bool on)
{
#if CONFIG_MIRROR_FLASH_GPIO >= 0
    if (on == s_flash_on) {
        return;
    }
    s_flash_on = on;
    flash_drive(on);
    if (s_flash_guard) {
        esp_timer_stop(s_flash_guard);
        if (on) {
            esp_timer_start_once(s_flash_guard, (uint64_t)FLASH_MAX_ON_MS * 1000);
        }
    }
#else
    (void)on;
#endif
}

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
