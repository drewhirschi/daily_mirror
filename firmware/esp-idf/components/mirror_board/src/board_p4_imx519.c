/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Portions adapted from the imx708_wifi_snapshot example in
 * mushBrainDave/esp32-p4-imx-camera (Apache-2.0); see ../../../NOTICE.
 *
 * Board adapter: Waveshare ESP32-P4 + Arducam IMX519 (AK7375 autofocus).
 *
 * MIPI-CSI through esp_video, the ISP doing AE/AWB/AF, and the P4's hardware
 * JPEG encoder. A capture task keeps the pipeline running and republishes the
 * newest RGB565 frame into PSRAM; captures encode from that copy.
 *
 * Serving straight from a dequeued V4L2 buffer would be simpler and wrong. The
 * camera does not stop while a response is on the wire, and with BUFFER_COUNT
 * buffers - one held by a handler - the driver runs out of places to put
 * incoming frames and recycles the one being read: clean at the top, junk at
 * the bottom. Keeping the stream running between captures also means AE, AWB
 * and autofocus stay converged, so a press returns a settled frame rather than
 * the first frame after a cold start.
 */
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <inttypes.h>
#include <sys/ioctl.h>
#include <sys/mman.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "driver/gpio.h"
#include "driver/ledc.h"
#include "driver/jpeg_encode.h"
#include "linux/videodev2.h"
#include "esp_video_init.h"
#include "esp_video_device.h"
#include "esp_video_ioctl.h"

#include "mirror_board.h"
#if CONFIG_MIRROR_FACE_AE
#include "face_ae.h"
#endif

static const char *TAG = "board_p4";

#define CAM_SCCB_I2C_PORT 0
#define CAM_SCCB_FREQ_HZ  100000
#define BUFFER_COUNT      2

static const esp_video_init_csi_config_t s_csi_config[] = {{
    .sccb_config = {
        .init_sccb = true,
        .i2c_config = {
            .port = CAM_SCCB_I2C_PORT,
            .scl_pin = CONFIG_MIRROR_CAM_SCCB_SCL_GPIO,
            .sda_pin = CONFIG_MIRROR_CAM_SCCB_SDA_GPIO,
        },
        .freq = CAM_SCCB_FREQ_HZ,
    },
    .reset_pin = -1,   /* the IMX519 module brings out neither */
    .pwdn_pin  = -1,
}};

/*
 * The AK7375 VCM is a separate chip from the sensor (I2C 0x0c on the same
 * bus), so esp_video probes for it separately and needs its own entry -
 * without one the motor auto-detect array is never walked and the lens never
 * moves, which looks exactly like a broken lens.
 */
static const esp_video_init_cam_motor_config_t s_motor_config[] = {{
    .sccb_config = {
        .init_sccb = true,
        .i2c_config = {
            .port = CAM_SCCB_I2C_PORT,
            .scl_pin = CONFIG_MIRROR_CAM_SCCB_SCL_GPIO,
            .sda_pin = CONFIG_MIRROR_CAM_SCCB_SDA_GPIO,
        },
        .freq = CAM_SCCB_FREQ_HZ,
    },
    .reset_pin  = -1,
    .pwdn_pin   = -1,
    .signal_pin = -1,
}};

static const esp_video_init_config_t s_cam_config = {
    .csi = s_csi_config,
    .cam_motor = s_motor_config,
};

/* The published frame. */
static uint8_t *s_frame;
static size_t s_frame_len;
static volatile uint32_t s_frame_seq;
static SemaphoreHandle_t s_frame_lock;
static uint32_t s_w, s_h;
static char s_status[72] = "camera not started";

/* The CSI device, for reading back what AE chose, and the latch of it. */
static int s_cam_fd = -1;
static int32_t s_exposure_us = -1;
static float s_analog_gain = -1.0f;
static int s_mean_luma = -1;
static int s_face_steered = -1;

struct capture_ctx {
    int fd;
    uint8_t **buffer;   /* the mmap'd V4L2 buffer table, indexed by buf.index */
};

static void capture_task(void *arg)
{
    struct capture_ctx *ctx = arg;
    const int type = V4L2_BUF_TYPE_VIDEO_CAPTURE;

    for (;;) {
        struct v4l2_buffer buf = { .type = type, .memory = V4L2_MEMORY_MMAP };
        if (ioctl(ctx->fd, VIDIOC_DQBUF, &buf) != 0) {
            ESP_LOGE(TAG, "DQBUF failed - capture task stopping");
            vTaskDelete(NULL);
            return;
        }
        if (xSemaphoreTake(s_frame_lock, pdMS_TO_TICKS(1000)) == pdTRUE) {
            memcpy(s_frame, ctx->buffer[buf.index], s_frame_len);
            s_frame_seq++;
            xSemaphoreGive(s_frame_lock);
        }
        /* Straight back to the driver: the publish above is a memcpy, so a
         * buffer is never held across anything slow. */
        ioctl(ctx->fd, VIDIOC_QBUF, &buf);
    }
}

#if CONFIG_MIRROR_FACE_AE
/* The face task's view of the frame: every step-th pixel of every step-th row,
 * from the middle of each step x step cell. A strided read of ~58 thousand
 * pixels, a few milliseconds under the lock - the capture task's next memcpy
 * waits that long at most. */
static bool face_grab(uint16_t *dst, uint32_t dw, uint32_t dh, uint32_t step)
{
    if (!s_frame || dw * step > s_w || dh * step > s_h ||
            xSemaphoreTake(s_frame_lock, pdMS_TO_TICKS(200)) != pdTRUE) {
        return false;
    }
    const uint16_t *px = (const uint16_t *)s_frame;
    for (uint32_t y = 0; y < dh; y++) {
        const uint16_t *row = px + (size_t)(y * step + step / 2) * s_w + step / 2;
        for (uint32_t x = 0; x < dw; x++) {
            dst[y * dw + x] = row[x * step];
        }
    }
    xSemaphoreGive(s_frame_lock);
    return true;
}
#endif

const char *board_name(void) { return "ESP32-P4 + IMX519"; }
const char *board_id(void)   { return "esp32p4-imx519"; }

esp_err_t board_camera_init(void)
{
    esp_err_t err = esp_video_init(&s_cam_config);
    if (err != ESP_OK) {
        /* Nearly always the camera ribbon on this rig: the log line just above
         * reads "Failed to detect camera sensor with address=1a". Reseat it,
         * or just reboot - it often comes up on the second try. */
        ESP_LOGE(TAG, "esp_video_init failed: %s", esp_err_to_name(err));
        return err;
    }

    int fd = open(ESP_VIDEO_MIPI_CSI_DEVICE_NAME, O_RDONLY);
    if (fd < 0) {
        ESP_LOGE(TAG, "open %s failed", ESP_VIDEO_MIPI_CSI_DEVICE_NAME);
        return ESP_ERR_NOT_FOUND;
    }

    const int type = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    struct v4l2_format fmt = { .type = type };
    ioctl(fd, VIDIOC_G_FMT, &fmt);
    s_w = fmt.fmt.pix.width;
    s_h = fmt.fmt.pix.height;
    s_frame_len = (size_t)s_w * s_h * 2;
    if (fmt.fmt.pix.pixelformat != V4L2_PIX_FMT_RGB565) {
        ESP_LOGE(TAG, "expected RGB565 out of the ISP - the JPEG will be garbage");
    }

    static uint8_t *buffer[BUFFER_COUNT];
    struct v4l2_requestbuffers req = { .count = BUFFER_COUNT, .type = type, .memory = V4L2_MEMORY_MMAP };
    ioctl(fd, VIDIOC_REQBUFS, &req);
    for (int i = 0; i < BUFFER_COUNT; i++) {
        struct v4l2_buffer b = { .type = type, .memory = V4L2_MEMORY_MMAP, .index = i };
        ioctl(fd, VIDIOC_QUERYBUF, &b);
        buffer[i] = mmap(NULL, b.length, PROT_READ | PROT_WRITE, MAP_SHARED, fd, b.m.offset);
        ioctl(fd, VIDIOC_QBUF, &b);
    }

    s_frame = heap_caps_malloc(s_frame_len, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    s_frame_lock = xSemaphoreCreateMutex();
    if (!s_frame || !s_frame_lock) {
        ESP_LOGE(TAG, "no PSRAM for the published frame (%u bytes)", (unsigned)s_frame_len);
        return ESP_ERR_NO_MEM;
    }

    ioctl(fd, VIDIOC_STREAMON, &type);
    s_cam_fd = fd;

    /* static, not a stack local: the task outlives this function. */
    static struct capture_ctx ctx;
    ctx.fd = fd;
    ctx.buffer = buffer;
    if (xTaskCreatePinnedToCore(capture_task, "capture", 4096, &ctx, 5, NULL,
                                tskNO_AFFINITY) != pdPASS) {
        ESP_LOGE(TAG, "could not start the capture task");
        return ESP_ERR_NO_MEM;
    }

    snprintf(s_status, sizeof(s_status), "IMX519 %" PRIu32 "x%" PRIu32 ", ISP AE/AWB/AF continuous", s_w, s_h);
    ESP_LOGI(TAG, "%s (%u bytes/frame)", s_status, (unsigned)s_frame_len);

#if CONFIG_MIRROR_FACE_AE
    /* The ISP's statistics windows are in sensor-mode coordinates, so the
     * face task needs those as well as the frame's. Not fatal: without it AE
     * and AF simply stay on the tuning file's windows. */
    esp_cam_sensor_format_t sf;
    uint32_t isp_w = s_w, isp_h = s_h;
    if (ioctl(fd, VIDIOC_G_SENSOR_FMT, &sf) == 0) {
        isp_w = sf.width;
        isp_h = sf.height;
    }
    err = face_ae_start(s_w, s_h, isp_w, isp_h, face_grab);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "face AE/AF not started: %s", esp_err_to_name(err));
    }
#endif
    return ESP_OK;
}

/*
 * What esp_ipa's AE last wrote to the IMX519, read back through the same V4L2
 * controls it writes: exposure in lines (x the mode's line time) and gain as a
 * menu index (whose menu value is milli-x). This is the setting in force for
 * the frame being encoded give or take the sensor's two-frame latency, which
 * is noise once AE has settled.
 */
static void latch_exposure(void)
{
    s_exposure_us = -1;
    s_analog_gain = -1.0f;

    struct v4l2_ext_control ctrl[2] = {
        { .id = V4L2_CID_EXPOSURE },
        { .id = V4L2_CID_GAIN },
    };
    struct v4l2_ext_controls ctrls = { .ctrl_class = V4L2_CID_CAMERA_CLASS, .count = 1, .controls = &ctrl[0] };
    esp_cam_sensor_format_t sf;
    if (ioctl(s_cam_fd, VIDIOC_G_EXT_CTRLS, &ctrls) == 0 &&
            ioctl(s_cam_fd, VIDIOC_G_SENSOR_FMT, &sf) == 0 && sf.isp_info) {
        s_exposure_us = (int32_t)(((uint64_t)ctrl[0].value * sf.isp_info->isp_v1_info.tline_ns) / 1000);
    }

    ctrls = (struct v4l2_ext_controls){ .ctrl_class = V4L2_CID_USER_CLASS, .count = 1, .controls = &ctrl[1] };
    if (ioctl(s_cam_fd, VIDIOC_G_EXT_CTRLS, &ctrls) == 0) {
        struct v4l2_querymenu qm = { .id = V4L2_CID_GAIN, .index = ctrl[1].value };
        if (ioctl(s_cam_fd, VIDIOC_QUERYMENU, &qm) == 0) {
            s_analog_gain = (float)qm.value / 1000.0f;
        }
    }
}

/* Mean luma (0-255) of the published RGB565 frame, from every 8th pixel of
 * every 8th row: 32 thousand samples, well under a millisecond. Caller holds
 * s_frame_lock. */
static int frame_mean_luma(void)
{
    const uint16_t *px = (const uint16_t *)s_frame;
    uint64_t sum = 0;
    uint32_t n = 0;
    for (uint32_t y = 4; y < s_h; y += 8) {
        for (uint32_t x = 4; x < s_w; x += 8) {
            uint16_t p = px[y * s_w + x];
            uint32_t r = (p >> 11) << 3, g = ((p >> 5) & 0x3f) << 2, b = (p & 0x1f) << 3;
            sum += (77 * r + 150 * g + 29 * b) >> 8;
            n++;
        }
    }
    return n ? (int)(sum / n) : -1;
}

esp_err_t board_camera_capture(bool autofocus, uint8_t **jpeg, size_t *len)
{
    *jpeg = NULL;
    *len = 0;
    if (!s_frame) {
        return ESP_ERR_INVALID_STATE;
    }

    if (autofocus) {
        /*
         * There is no single-shot AF to trigger here: the ISP's contrast-detect
         * loop has been hill-climbing the whole time the stream has been up.
         * What it does need is fresh frames to score, so wait for a handful
         * rather than encoding whatever was published a moment ago.
         */
        uint32_t target = s_frame_seq + 8;
        for (int i = 0; i < 60 && s_frame_seq < target; i++) {
            vTaskDelay(pdMS_TO_TICKS(20));
        }
    }

    jpeg_encoder_handle_t enc = NULL;
    latch_exposure();
#if CONFIG_MIRROR_FACE_AE
    s_face_steered = face_ae_steering() ? 1 : 0;
#endif
    jpeg_encode_engine_cfg_t eng = { .timeout_ms = 5000 };
    if (jpeg_new_encoder_engine(&eng, &enc) != ESP_OK) {
        return ESP_FAIL;
    }

    /* Half the raw size; a q90 frame is comfortably under it. */
    jpeg_encode_memory_alloc_cfg_t mem = { .buffer_direction = JPEG_ENC_ALLOC_OUTPUT_BUFFER };
    size_t alloc = 0;
    uint8_t *out = jpeg_alloc_encoder_mem((size_t)s_w * s_h, &mem, &alloc);
    if (!out) {
        jpeg_del_encoder_engine(enc);
        return ESP_ERR_NO_MEM;
    }

    /*
     * Subsampling is YUV422, not the more usual 420: 1080 is not a multiple of
     * the 16-pixel MCU height 420 needs (it is a multiple of 8, so 422 divides
     * cleanly), and 422 keeps more chroma detail.
     */
    jpeg_encode_cfg_t cfg = {
        .width = s_w,
        .height = s_h,
        .src_type = JPEG_ENCODE_IN_FORMAT_RGB565,
        .sub_sample = JPEG_DOWN_SAMPLING_YUV422,
        .image_quality = CONFIG_MIRROR_JPEG_QUALITY,
    };
    uint32_t out_len = 0;
    esp_err_t ret = ESP_ERR_TIMEOUT;
    /* Encode under the lock - tens of milliseconds, so at most a dropped
     * frame. Everything slower happens after it is released. */
    if (xSemaphoreTake(s_frame_lock, pdMS_TO_TICKS(3000)) == pdTRUE) {
        s_mean_luma = frame_mean_luma();
        ret = jpeg_encoder_process(enc, &cfg, s_frame, s_frame_len, out, alloc, &out_len);
        xSemaphoreGive(s_frame_lock);
    }
    ESP_LOGI(TAG, "capture: exposure %" PRIi32 " us, gain %.2fx, mean luma %d, face-steered %d",
             s_exposure_us, s_analog_gain, s_mean_luma, s_face_steered);
    jpeg_del_encoder_engine(enc);

    if (ret != ESP_OK) {
        free(out);
        return ret;
    }
    *jpeg = out;
    *len = out_len;
    return ESP_OK;
}

void board_camera_prepare_focus(uint32_t budget_ms)
{
    /* Nothing to arrange. The capture task keeps the pipeline running between
     * presses, so AE, AWB and the ISP's continuous autofocus are already
     * converged - which is the state the S3 has to spend a countdown getting
     * itself into. */
    (void)budget_ms;
}

void board_camera_prepare_settle(uint32_t budget_ms)
{
    (void)budget_ms;
}

/* No flash on this board yet: MIRROR_FLASH_GPIO defaults to -1 for it. */
bool board_flash_available(void)          { return false; }
bool board_flash_on(void)                 { return false; }
void board_flash_set(bool on)             { (void)on; }
esp_err_t board_flash_init(void)          { return ESP_OK; }

void board_camera_release(uint8_t *jpeg)
{
    free(jpeg);
}

void board_camera_last_size(uint32_t *w, uint32_t *h)
{
    if (w) *w = s_w;
    if (h) *h = s_h;
}

const char *board_camera_status(void) { return s_status; }

/*
 * Capture provenance, P4 edition: size, quality, and the exposure, gain and
 * mean luma latched by the last board_camera_capture(), and whether a face was
 * steering AE/AF when it was taken. The AF state is still
 * an honest "unknown": it lives in esp_ipa's closed-loop AF on the AK7375
 * rather than in anything readable, and reporting a made-up value would be
 * worse than reporting nothing - the upload omits fields left at these values.
 */
void board_camera_capture_info(board_camera_capture_info_t *out)
{
    if (!out) {
        return;
    }
    *out = (board_camera_capture_info_t){
        .valid = true,
        .sensor = "imx519",
        .width = s_w,
        .height = s_h,
        .jpeg_quality = CONFIG_MIRROR_JPEG_QUALITY,
        .exposure_us = s_exposure_us,
        .analog_gain = s_analog_gain,
        .mean_luma = s_mean_luma,
        .af_state = "unknown",
        .focus_score = -1,
        .flash = false,
        .face_steered = s_face_steered,
    };
}

int board_camera_face_stats(char *out, size_t cap)
{
#if CONFIG_MIRROR_FACE_AE
    return face_ae_stats(out, cap);
#else
    (void)out;
    (void)cap;
    return 0;
#endif
}

esp_err_t board_camera_face_test(const uint8_t *jpeg, size_t len, bool steer, char *out, size_t cap)
{
#if CONFIG_MIRROR_FACE_AE
    return face_ae_test_jpeg(jpeg, len, steer, out, cap);
#else
    (void)jpeg;
    (void)len;
    (void)steer;
    snprintf(out, cap, "face detection is not built in (CONFIG_MIRROR_FACE_AE)\n");
    return ESP_ERR_NOT_SUPPORTED;
#endif
}

/* ---- RGB LED on three LEDC PWM channels -------------------------------- */
/*
 * LEDC_TIMER_0 / LEDC_CHANNEL_0 are deliberately left alone: esp32-camera uses
 * them for XCLK on the other board, and keeping the two adapters off each
 * other's numbering makes the pair easier to reason about.
 */
static const int s_led_gpio[3] = {
    CONFIG_MIRROR_LED_R_GPIO, CONFIG_MIRROR_LED_G_GPIO, CONFIG_MIRROR_LED_B_GPIO
};
static const ledc_channel_t s_led_ch[3] = { LEDC_CHANNEL_1, LEDC_CHANNEL_2, LEDC_CHANNEL_3 };

#if CONFIG_MIRROR_LED_COMMON_ANODE
#define LED_INVERTED 1
#else
#define LED_INVERTED 0
#endif

esp_err_t board_led_init(void)
{
    ledc_timer_config_t t = {
        .speed_mode = LEDC_LOW_SPEED_MODE, .duty_resolution = LEDC_TIMER_8_BIT,
        .timer_num = LEDC_TIMER_1, .freq_hz = 2000, .clk_cfg = LEDC_AUTO_CLK,
    };
    esp_err_t err = ledc_timer_config(&t);
    if (err != ESP_OK) {
        return err;
    }
    for (int i = 0; i < 3; i++) {
        ledc_channel_config_t c = {
            .gpio_num = s_led_gpio[i], .speed_mode = LEDC_LOW_SPEED_MODE,
            .channel = s_led_ch[i], .timer_sel = LEDC_TIMER_1,
            .duty = LED_INVERTED ? 255 : 0, .hpoint = 0,
        };
        err = ledc_channel_config(&c);
        if (err != ESP_OK) {
            return err;
        }
    }
    ESP_LOGI(TAG, "RGB LED on GPIO %d/%d/%d, common %s",
             s_led_gpio[0], s_led_gpio[1], s_led_gpio[2],
             LED_INVERTED ? "anode (3V3)" : "cathode (GND)");
    return ESP_OK;
}

void board_led_set_rgb(uint8_t r, uint8_t g, uint8_t b)
{
    const uint8_t v[3] = { r, g, b };
    for (int i = 0; i < 3; i++) {
        uint32_t duty = LED_INVERTED ? 255u - v[i] : v[i];
        ledc_set_duty(LEDC_LOW_SPEED_MODE, s_led_ch[i], duty);
        ledc_update_duty(LEDC_LOW_SPEED_MODE, s_led_ch[i]);
    }
}

int board_button_gpio(void) { return CONFIG_MIRROR_BUTTON_GPIO; }

esp_err_t board_net_init(void)
{
    /*
     * Nothing to do here in code: esp_wifi_remote replaces the esp_wifi API at
     * link time and esp_hosted brings the SDIO transport up inside
     * esp_wifi_init(). This exists to say so, and to give the failure a name -
     * if esp_wifi_init() then returns an error it is the C6 not answering,
     * which is a radio problem, not a network one.
     */
    ESP_LOGI(TAG, "Wi-Fi is on the ESP32-C6 via esp_hosted over SDIO");
    return ESP_OK;
}
