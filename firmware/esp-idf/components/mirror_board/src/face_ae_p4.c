/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Face-steered AE and AF for the P4 + IMX519.
 *
 * The tuning JSON meters with a centre-weighted table and focuses on three
 * windows across the middle of the frame, which is right for someone standing
 * square in front of the mirror and wrong for anyone a little to one side: AE
 * then exposes for the wall behind them and AF focuses on it. This task finds
 * the face and moves the ISP's AE and AF statistics windows onto it, and puts
 * the defaults back once nobody has been seen for FACE_TIMEOUT_MS.
 *
 * It does not touch the frame the camera is producing. Every FACE_PERIOD_MS it
 * takes a 320-wide decimated copy of the published frame (a few ms under the
 * frame lock, a strided read - no scaler, no second pipeline output), runs
 * ESP-DL's face detector on that copy outside the lock, and writes two V4L2
 * controls. esp_ipa's AE/AF loops carry on as before; they just read their
 * statistics from a different rectangle. AWB is left on the whole frame, since
 * a face is a poor grey-world sample.
 *
 * Coordinates. The windows are in the ISP's input space: the sensor mode as it
 * arrives over CSI, i.e. after the IMX519's 2x2 binning and its own crop, and
 * after its flip/mirror, which the sensor applies at readout. The published
 * frame is the ISP's output and this P4 (rev v1.3) has no ISP crop, so the two
 * are the same 1920x1080 and a detection maps across by the decimation step
 * alone. Should the sensor mode ever be larger than the frame, the difference
 * is taken as a centred crop and added back as an offset.
 *
 * Memory. The task stack, the decimated copy, the model parameters and its
 * activations are all PSRAM. What lands in internal RAM is ESP-DL's object
 * graph, ~15 KB of small C++ allocations (see face_task), logged at boot.
 */
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <inttypes.h>
#include <sys/ioctl.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "freertos/idf_additions.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "esp_heap_caps.h"
#include "driver/jpeg_decode.h"
#include "linux/videodev2.h"
#include "esp_video_device.h"
#include "esp_video_isp_ioctl.h"
#include "esp_video_isp_pipeline.h"

#include "face_ae.h"
#include "face_detect.h"

static const char *TAG = "face_ae";

#define FACE_PERIOD_MS     400    /* between detections */
#define FACE_TIMEOUT_MS    2000   /* no face for this long: back to the defaults */
#define FACE_DET_WIDTH     320    /* decimated copy; the model itself looks at 160x120 */
#define FACE_MAX           4
#define FACE_SMOOTH        0.5f   /* EMA weight of a new detection */
#define FACE_RESTEER_MS    800    /* a moving face re-steers at most this often */

/*
 * Window sizing. AE grows the face a little so the 5x5 metering grid, whose
 * centre-weighted table still applies inside the window, spans forehead to
 * chin; AF grows it a little more to take in the hairline and jaw, which is
 * where the edge energy is. The minimums keep a distant face from producing a
 * window too small to meter or score sensibly.
 */
#define AE_GROW            1.3f
#define AF_GROW            1.4f
#define AE_MIN_PX          240
#define AF_MIN_PX          192

static uint32_t s_fw, s_fh;           /* the published frame */
static uint32_t s_iw, s_ih;           /* the ISP input */
static uint32_t s_off_x, s_off_y;     /* frame -> ISP input */
static face_ae_grab_fn s_grab;
static TaskHandle_t s_task;

static uint16_t *s_small;
static uint32_t s_sw, s_sh, s_step;

static int s_isp_fd = -1;
static esp_video_isp_ae_t s_ae_default;
static esp_video_isp_af_t s_af_default;
static bool s_have_defaults;

/* Tracker and stats. Written only by the face task; read by /stats under the mux. */
static portMUX_TYPE s_mux = portMUX_INITIALIZER_UNLOCKED;
static struct {
    bool have;
    float cx, cy, w, h;     /* frame coordinates, smoothed */
    float score;
    int64_t seen_us;
} s_trk;
static bool s_steering;
static isp_window_t s_ae_win, s_af_win;
static int64_t s_steer_us;
static uint32_t s_detect_ms;
static uint32_t s_runs, s_hits, s_steers;
static bool s_ready;

/* The /debug/face hand-off: one job at a time, run on the face task so the
 * detector is only ever entered from one place. */
static SemaphoreHandle_t s_job_lock, s_job_done;
static struct {
    const uint8_t *jpeg;
    size_t len;
    bool steer;
    esp_err_t ret;
    char out[512];
} s_job;
static volatile bool s_job_pending;

static isp_window_t make_window(uint32_t l, uint32_t t, uint32_t w, uint32_t h)
{
    return (isp_window_t){
        .top_left = { .x = l, .y = t },
        .btm_right = { .x = l + w - 1, .y = t + h - 1 },
    };
}

/*
 * The tracked face, grown and clamped into a window in ISP coordinates. Sizes
 * are rounded to multiples of 10 so AE's 5x5 grid divides evenly, and corners
 * to even pixels so the window starts on a Bayer quad.
 */
static isp_window_t face_window(float grow, uint32_t min_px)
{
    float w = s_trk.w * grow, h = s_trk.h * grow;
    if (w < min_px) w = min_px;
    if (h < min_px) h = min_px;
    if (w > s_iw) w = s_iw;
    if (h > s_ih) h = s_ih;
    uint32_t ww = ((uint32_t)w / 10) * 10, hh = ((uint32_t)h / 10) * 10;

    float l = s_trk.cx + s_off_x - ww / 2.0f, t = s_trk.cy + s_off_y - hh / 2.0f;
    if (l < 0) l = 0;
    if (t < 0) t = 0;
    if (l + ww > s_iw) l = s_iw - ww;
    if (t + hh > s_ih) t = s_ih - hh;
    return make_window((uint32_t)l & ~1u, (uint32_t)t & ~1u, ww, hh);
}

static uint32_t win_w(const isp_window_t *w) { return w->btm_right.x - w->top_left.x + 1; }
static uint32_t win_h(const isp_window_t *w) { return w->btm_right.y - w->top_left.y + 1; }

/* Worth re-steering for: the centre has moved by 15% of the window, or the
 * size by 20%. Anything less is detector jitter, and every write restarts the
 * ISP's statistics block and hands AF a step change in its score. */
static bool moved(const isp_window_t *a, const isp_window_t *b)
{
    int32_t dx = (int32_t)(a->top_left.x + a->btm_right.x) / 2 - (int32_t)(b->top_left.x + b->btm_right.x) / 2;
    int32_t dy = (int32_t)(a->top_left.y + a->btm_right.y) / 2 - (int32_t)(b->top_left.y + b->btm_right.y) / 2;
    int32_t dw = (int32_t)win_w(a) - (int32_t)win_w(b), dh = (int32_t)win_h(a) - (int32_t)win_h(b);
    return abs(dx) * 100 > 15 * (int32_t)win_w(b) || abs(dy) * 100 > 15 * (int32_t)win_h(b) ||
           abs(dw) * 100 > 20 * (int32_t)win_w(b) || abs(dh) * 100 > 20 * (int32_t)win_h(b);
}

static esp_err_t set_window(uint32_t which, const isp_window_t *w)
{
    return esp_video_isp_pipeline_set_statistics_window(which, w->top_left.x, w->top_left.y, win_w(w), win_h(w));
}

/*
 * Back to what the ISP held before the first steer (on this build the full
 * frame for AE and for all three AF windows). AE goes through the pipeline
 * call like the steering does. AF does not: that call writes one rectangle
 * into all three AF windows, and the defaults need not be one rectangle, so it
 * is a direct control write - outside the pipeline's lock, which is harmless
 * because the pipeline only writes AF when the IPA asks it to, and with this
 * tuning it never does after start-up (/stats reads the windows back).
 */
static void restore_defaults(void)
{
    set_window(ESP_VIDEO_ISP_AE_STATS_WIN, &s_ae_default.windows[0]);

    esp_video_isp_af_t af;
    struct v4l2_ext_control c = { .id = V4L2_CID_USER_ESP_ISP_AF, .p_u8 = (uint8_t *)&af };
    struct v4l2_ext_controls cs = { .ctrl_class = V4L2_CID_USER_CLASS, .count = 1, .controls = &c };
    if (ioctl(s_isp_fd, VIDIOC_G_EXT_CTRLS, &cs) == 0) {
        memcpy(af.windows, s_af_default.windows, sizeof(af.windows));
        if (ioctl(s_isp_fd, VIDIOC_S_EXT_CTRLS, &cs) != 0) {
            ESP_LOGW(TAG, "could not restore the AF windows");
        }
    }
}

static bool read_isp(uint32_t id, void *p)
{
    struct v4l2_ext_control c = { .id = id, .p_u8 = p };
    struct v4l2_ext_controls cs = { .ctrl_class = V4L2_CID_USER_CLASS, .count = 1, .controls = &c };
    return s_isp_fd >= 0 && ioctl(s_isp_fd, VIDIOC_G_EXT_CTRLS, &cs) == 0;
}

/* One detection result (or none) into the tracker, in frame coordinates. */
static void track(const face_box_t *f, int64_t now)
{
    portENTER_CRITICAL(&s_mux);
    if (f) {
        float cx = f->x + f->w / 2.0f, cy = f->y + f->h / 2.0f;
        float big = f->w > s_trk.w ? f->w : s_trk.w;
        bool jump = !s_trk.have || fabsf(cx - s_trk.cx) > big / 2 || fabsf(cy - s_trk.cy) > big / 2 ||
                    f->w > s_trk.w * 1.6f || f->w * 1.6f < s_trk.w;
        if (jump) {
            s_trk.cx = cx; s_trk.cy = cy; s_trk.w = f->w; s_trk.h = f->h;
        } else {
            s_trk.cx += FACE_SMOOTH * (cx - s_trk.cx);
            s_trk.cy += FACE_SMOOTH * (cy - s_trk.cy);
            s_trk.w += FACE_SMOOTH * (f->w - s_trk.w);
            s_trk.h += FACE_SMOOTH * (f->h - s_trk.h);
        }
        s_trk.score = f->score;
        s_trk.seen_us = now;
        s_trk.have = true;
        s_hits++;
    } else if (s_trk.have && now - s_trk.seen_us > (int64_t)FACE_TIMEOUT_MS * 1000) {
        s_trk.have = false;
    }
    portEXIT_CRITICAL(&s_mux);
}

static void steer(int64_t now)
{
    if (!s_have_defaults) {
        return;
    }
    if (s_trk.have) {
        isp_window_t ae = face_window(AE_GROW, AE_MIN_PX), af = face_window(AF_GROW, AF_MIN_PX);
        bool due = !s_steering || (now - s_steer_us > (int64_t)FACE_RESTEER_MS * 1000 &&
                                   (moved(&ae, &s_ae_win) || moved(&af, &s_af_win)));
        if (!due) {
            return;
        }
        esp_err_t e1 = set_window(ESP_VIDEO_ISP_AE_STATS_WIN, &ae);
        esp_err_t e2 = set_window(ESP_VIDEO_ISP_AF_STATS_WIN, &af);
        if (e1 != ESP_OK || e2 != ESP_OK) {
            ESP_LOGW(TAG, "steer failed: AE %s, AF %s", esp_err_to_name(e1), esp_err_to_name(e2));
            return;
        }
        if (!s_steering) {
            ESP_LOGI(TAG, "face: steering AE/AF");
        }
        ESP_LOGI(TAG, "AE window %" PRIu32 ",%" PRIu32 " %" PRIu32 "x%" PRIu32 ", AF %" PRIu32 ",%" PRIu32
                 " %" PRIu32 "x%" PRIu32, ae.top_left.x, ae.top_left.y, win_w(&ae), win_h(&ae),
                 af.top_left.x, af.top_left.y, win_w(&af), win_h(&af));
        portENTER_CRITICAL(&s_mux);
        s_ae_win = ae;
        s_af_win = af;
        s_steering = true;
        s_steers++;
        portEXIT_CRITICAL(&s_mux);
        s_steer_us = now;
    } else if (s_steering) {
        restore_defaults();
        portENTER_CRITICAL(&s_mux);
        s_steering = false;
        portEXIT_CRITICAL(&s_mux);
        ESP_LOGI(TAG, "no face for %d ms: AE/AF back to the defaults", FACE_TIMEOUT_MS);
    }
}

/* Largest face first: in a mirror that is the person using it, not someone
 * walking past behind them. */
static const face_box_t *largest(const face_box_t *f, int n)
{
    const face_box_t *best = NULL;
    for (int i = 0; i < n; i++) {
        if (!best || f[i].w * f[i].h > best->w * best->h) {
            best = &f[i];
        }
    }
    return best;
}

/*
 * The hardware decoder, not a software one: esp_new_jpeg takes tens of KB of
 * internal RAM for its working buffers on a 1080p image, which is exactly the
 * headroom an upload running at the same moment needs. The output is padded
 * to whole 16x16 blocks; `*stride` is the padded width.
 */
static esp_err_t decode_jpeg(const uint8_t *jpeg, size_t len, uint16_t **px, int *w, int *h, int *stride)
{
    jpeg_decode_picture_info_t info;
    esp_err_t ret = jpeg_decoder_get_info(jpeg, len, &info);
    if (ret != ESP_OK) {
        return ret;
    }
    uint32_t pw = (info.width + 15) & ~15u, ph = (info.height + 15) & ~15u;
    jpeg_decode_memory_alloc_cfg_t mem = { .buffer_direction = JPEG_DEC_ALLOC_OUTPUT_BUFFER };
    size_t alloc = 0;
    uint8_t *out = jpeg_alloc_decoder_mem((size_t)pw * ph * 2, &mem, &alloc);
    if (!out) {
        return ESP_ERR_NO_MEM;
    }
    jpeg_decoder_handle_t dec = NULL;
    jpeg_decode_engine_cfg_t eng = { .timeout_ms = 2000 };
    ret = jpeg_new_decoder_engine(&eng, &dec);
    if (ret == ESP_OK) {
        /* BGR element order is the little-endian RGB565 the ISP produces and
         * the detector is told to expect. */
        jpeg_decode_cfg_t cfg = {
            .output_format = JPEG_DECODE_OUT_FORMAT_RGB565,
            .rgb_order = JPEG_DEC_RGB_ELEMENT_ORDER_BGR,
            .conv_std = JPEG_YUV_RGB_CONV_STD_BT601,
        };
        uint32_t out_len = 0;
        ret = jpeg_decoder_process(dec, &cfg, jpeg, len, out, alloc, &out_len);
        jpeg_del_decoder_engine(dec);
    }
    if (ret != ESP_OK) {
        free(out);
        return ret;
    }
    *px = (uint16_t *)out;
    *w = info.width;
    *h = info.height;
    *stride = pw;
    return ESP_OK;
}

static void run_job(void)
{
    char *o = s_job.out;
    size_t cap = sizeof(s_job.out);
    int n = 0;
    uint16_t *img = NULL, *small = NULL;
    int w = 0, h = 0, stride = 0;

    s_job.ret = decode_jpeg(s_job.jpeg, s_job.len, &img, &w, &h, &stride);
    if (s_job.ret != ESP_OK) {
        snprintf(o, cap, "decode failed: %s\n", esp_err_to_name(s_job.ret));
        return;
    }
    /* Decimated to the same width the live path feeds the detector, so this
     * measures what the camera would see. Width a multiple of 8 keeps each row
     * 16-byte aligned for the detector's SIMD resize. */
    uint32_t step = (w + FACE_DET_WIDTH - 1) / FACE_DET_WIDTH;
    if (step < 1) step = 1;
    uint32_t sw = (w / step) & ~7u, sh = h / step;
    small = heap_caps_aligned_alloc(16, (size_t)sw * sh * 2, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!small || !sw || !sh) {
        free(img);
        heap_caps_free(small);
        s_job.ret = ESP_ERR_NO_MEM;
        snprintf(o, cap, "no memory\n");
        return;
    }
    uint64_t sum_r = 0, sum_b = 0;
    for (uint32_t y = 0; y < sh; y++) {
        const uint16_t *row = img + (size_t)(y * step + step / 2) * stride + step / 2;
        for (uint32_t x = 0; x < sw; x++) {
            uint16_t p = row[x * step];
            small[y * sw + x] = p;
            sum_r += p >> 11;
            sum_b += p & 0x1f;
        }
    }
    free(img);

    face_box_t f[FACE_MAX];
    int64_t t0 = esp_timer_get_time();
    int found = face_detect_rgb565(small, sw, sh, f, FACE_MAX);
    uint32_t ms = (uint32_t)((esp_timer_get_time() - t0 + 500) / 1000);
    heap_caps_free(small);

    /* Mean red and blue (0-31), so a byte-order mistake in the decode shows
     * up as the two swapped rather than as a mysteriously worse detector. */
    n += snprintf(o + n, cap - n, "image=%dx%d\ndetector_input=%" PRIu32 "x%" PRIu32 "\nmean_r_b=%u,%u\n"
                  "faces=%d\ndetect_ms=%" PRIu32 "\n", w, h, sw, sh,
                  (unsigned)(sum_r / ((uint64_t)sw * sh)), (unsigned)(sum_b / ((uint64_t)sw * sh)), found, ms);
    for (int i = 0; i < found; i++) {
        f[i].x *= step; f[i].y *= step; f[i].w *= step; f[i].h *= step;
        n += snprintf(o + n, cap - n, "face%d=%d,%d,%d,%d score=%.2f\n", i, f[i].x, f[i].y, f[i].w, f[i].h, f[i].score);
    }
    const face_box_t *big = largest(f, found);
    if (s_job.steer && big) {
        /* Into frame coordinates by proportion, then through the same tracker
         * and steering as a live detection. */
        face_box_t fb = {
            .x = big->x * (int)s_fw / w, .y = big->y * (int)s_fh / h,
            .w = big->w * (int)s_fw / w, .h = big->h * (int)s_fh / h, .score = big->score,
        };
        int64_t now = esp_timer_get_time();
        track(&fb, now);
        steer(now);
        n += snprintf(o + n, cap - n, "steered=%d,%d,%d,%d\n", fb.x, fb.y, fb.w, fb.h);
    }
    s_job.ret = ESP_OK;
}

static void face_task(void *arg)
{
    (void)arg;
    /*
     * The model loads on this task, ~100 ms. Loaded from the caller's task
     * instead (board_camera_init, main task) it took ~1 s and the board then
     * watchdog-reset on the first detection, every boot - cause not run down.
     *
     * ESP-DL builds its graph out of many small C++ allocations, and under
     * SPIRAM_MALLOC_ALWAYSINTERNAL each one under 16 KB lands in internal RAM:
     * ~15 KB in all, logged below. Dropping the threshold around the load
     * with heap_caps_malloc_extmem_enable(0) is not an option: it is global,
     * and esp_hosted's SDIO tasks are already running and allocating.
     */
    size_t in0 = heap_caps_get_free_size(MALLOC_CAP_INTERNAL), ps0 = heap_caps_get_free_size(MALLOC_CAP_SPIRAM);
    int64_t t0 = esp_timer_get_time();
    if (face_detect_init() != ESP_OK) {
        ESP_LOGE(TAG, "face model did not load - AE/AF stay on the defaults");
        vTaskDeleteWithCaps(NULL);
        return;
    }
    ESP_LOGI(TAG, "model loaded in %" PRIi64 " ms: internal %u -> %u, PSRAM %u -> %u",
             (esp_timer_get_time() - t0) / 1000, (unsigned)in0,
             (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL), (unsigned)ps0,
             (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    /*
     * The defaults are read, not assumed: whatever the ISP holds once the
     * stream has been running a while - which is not necessarily what the
     * tuning JSON says (on this build the AF windows read back as the full
     * frame, not the JSON's three centre windows). The model loads in ~100 ms,
     * too soon to be sure the IPA has finished configuring, hence the wait.
     */
    vTaskDelay(pdMS_TO_TICKS(3000));
    s_isp_fd = open(ESP_VIDEO_ISP1_DEVICE_NAME, O_RDWR);
    if (s_isp_fd >= 0 && read_isp(V4L2_CID_USER_ESP_ISP_AE, &s_ae_default) &&
            read_isp(V4L2_CID_USER_ESP_ISP_AF, &s_af_default)) {
        s_have_defaults = true;
        const isp_window_t *a = &s_ae_default.windows[0];
        ESP_LOGI(TAG, "default AE window %" PRIu32 ",%" PRIu32 " %" PRIu32 "x%" PRIu32,
                 a->top_left.x, a->top_left.y, win_w(a), win_h(a));
        for (int i = 0; i < ISP_AF_WINDOW_NUM; i++) {
            const isp_window_t *f = &s_af_default.windows[i];
            ESP_LOGI(TAG, "default AF window %d %" PRIu32 ",%" PRIu32 " %" PRIu32 "x%" PRIu32, i,
                     f->top_left.x, f->top_left.y, win_w(f), win_h(f));
        }
    } else {
        ESP_LOGE(TAG, "cannot read the ISP's AE/AF windows - detecting, but not steering");
    }
    s_ready = true;

    bool first = true;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(FACE_PERIOD_MS));
        if (s_job_pending) {
            run_job();
            s_job_pending = false;
            xSemaphoreGive(s_job_done);
        }

        if (!s_grab(s_small, s_sw, s_sh, s_step)) {
            continue;
        }
        face_box_t f[FACE_MAX];
        int64_t t1 = esp_timer_get_time();
        int n = face_detect_rgb565(s_small, s_sw, s_sh, f, FACE_MAX);
        int64_t now = esp_timer_get_time();
        const face_box_t *big = largest(f, n);
        face_box_t fb;
        if (big) {
            fb = (face_box_t){ .x = big->x * (int)s_step, .y = big->y * (int)s_step,
                               .w = big->w * (int)s_step, .h = big->h * (int)s_step, .score = big->score };
        }
        portENTER_CRITICAL(&s_mux);
        s_detect_ms = (uint32_t)((now - t1 + 500) / 1000);
        s_runs++;
        portEXIT_CRITICAL(&s_mux);
        if (first) {
            ESP_LOGI(TAG, "first detection: %" PRIu32 " ms, internal free %u", s_detect_ms,
                     (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL));
            first = false;
        }

        bool had = s_trk.have;
        track(big ? &fb : NULL, now);
        if (s_trk.have != had) {
            if (s_trk.have) {
                ESP_LOGI(TAG, "face at %d,%d %dx%d (score %.2f, %" PRIu32 " ms)", fb.x, fb.y, fb.w, fb.h,
                         fb.score, s_detect_ms);
            } else {
                ESP_LOGI(TAG, "face lost");
            }
        }
        steer(now);
    }
}

esp_err_t face_ae_start(uint32_t frame_w, uint32_t frame_h, uint32_t isp_w, uint32_t isp_h,
                        face_ae_grab_fn grab)
{
    if (s_task || !grab || !frame_w || !frame_h) {
        return ESP_ERR_INVALID_STATE;
    }
    s_fw = frame_w;
    s_fh = frame_h;
    s_iw = isp_w >= frame_w ? isp_w : frame_w;
    s_ih = isp_h >= frame_h ? isp_h : frame_h;
    s_off_x = (s_iw - frame_w) / 2;
    s_off_y = (s_ih - frame_h) / 2;
    s_grab = grab;

    s_step = (frame_w + FACE_DET_WIDTH - 1) / FACE_DET_WIDTH;
    s_sw = (frame_w / s_step) & ~7u;
    s_sh = frame_h / s_step;
    s_small = heap_caps_aligned_alloc(16, (size_t)s_sw * s_sh * 2, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    s_job_lock = xSemaphoreCreateMutex();
    s_job_done = xSemaphoreCreateBinary();
    if (!s_small || !s_job_lock || !s_job_done) {
        return ESP_ERR_NO_MEM;
    }

    /* Below the capture task (5) and the HTTP server, and with its stack in
     * PSRAM: the internal RAM a 16 KB stack would take is most of what a TLS
     * upload has spare. A PSRAM stack only means the task cannot run while the
     * flash cache is off, which it never needs to. */
    if (xTaskCreatePinnedToCoreWithCaps(face_task, "face_ae", 16384, NULL, 2, &s_task, tskNO_AFFINITY,
                                        MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT) != pdPASS) {
        s_task = NULL;
        return ESP_ERR_NO_MEM;
    }
    if (s_off_x || s_off_y) {
        ESP_LOGW(TAG, "ISP input %" PRIu32 "x%" PRIu32 " is larger than the frame; assuming a centred crop",
                 s_iw, s_ih);
    }
    ESP_LOGI(TAG, "face AE/AF: %" PRIu32 "x%" PRIu32 " every %d ms, step %" PRIu32, s_sw, s_sh,
             FACE_PERIOD_MS, s_step);
    return ESP_OK;
}

bool face_ae_steering(void)
{
    return s_steering;
}

int face_ae_stats(char *out, size_t cap)
{
    portENTER_CRITICAL(&s_mux);
    bool have = s_trk.have, steering = s_steering;
    float cx = s_trk.cx, cy = s_trk.cy, w = s_trk.w, h = s_trk.h, score = s_trk.score;
    int64_t seen = s_trk.seen_us;
    uint32_t ms = s_detect_ms, runs = s_runs, hits = s_hits, steers = s_steers;
    portEXIT_CRITICAL(&s_mux);

    long age = seen ? (long)((esp_timer_get_time() - seen) / 1000) : -1;
    int n = snprintf(out, cap,
        "face=%d\n" "face_box=%d,%d,%d,%d\n" "face_score=%.2f\n" "face_age_ms=%ld\n"
        "detect_ms=%" PRIu32 "\n" "face_runs=%" PRIu32 "\n" "face_hits=%" PRIu32 "\n"
        "face_steering=%d\n" "face_steers=%" PRIu32 "\n",
        have ? 1 : 0, (int)(cx - w / 2), (int)(cy - h / 2), (int)w, (int)h, score, age,
        ms, runs, hits, steering ? 1 : 0, steers);

    /* What the ISP actually holds, read back rather than remembered, so a
     * write the IPA later overwrote would show here. */
    esp_video_isp_ae_t ae;
    esp_video_isp_af_t af;
    if (s_ready && read_isp(V4L2_CID_USER_ESP_ISP_AE, &ae) && read_isp(V4L2_CID_USER_ESP_ISP_AF, &af)) {
        const isp_window_t *a = &ae.windows[0], *f = &af.windows[0];
        n += snprintf(out + n, cap - n,
            "isp_ae_window=%" PRIu32 ",%" PRIu32 ",%" PRIu32 ",%" PRIu32 "\n"
            "isp_af_window0=%" PRIu32 ",%" PRIu32 ",%" PRIu32 ",%" PRIu32 "\n",
            a->top_left.x, a->top_left.y, win_w(a), win_h(a),
            f->top_left.x, f->top_left.y, win_w(f), win_h(f));
    }
    /* The luma AE is metering, from inside whichever window it has: the
     * evidence that steering moved what AE sees, not just a register. */
    float luma;
    if (s_ready && esp_video_isp_pipeline_get_env_float("ae.luma.avg", &luma) == ESP_OK) {
        n += snprintf(out + n, cap - n, "ae_luma=%.1f\n", luma);
    }
    return n;
}

esp_err_t face_ae_test_jpeg(const uint8_t *jpeg, size_t len, bool steer_it, char *out, size_t cap)
{
    if (!s_task || !s_ready) {
        snprintf(out, cap, "face detector not running\n");
        return ESP_ERR_INVALID_STATE;
    }
    if (xSemaphoreTake(s_job_lock, pdMS_TO_TICKS(5000)) != pdTRUE) {
        snprintf(out, cap, "busy\n");
        return ESP_ERR_TIMEOUT;
    }
    s_job.jpeg = jpeg;
    s_job.len = len;
    s_job.steer = steer_it;
    s_job.out[0] = '\0';
    s_job_pending = true;
    xTaskNotifyGive(s_task);
    /* A full-size JPEG decodes in well under a second in software. If this
     * ever times out the job is still pending and the caller's buffer must
     * stay alive, so the lock is kept and every later call reports busy. */
    if (xSemaphoreTake(s_job_done, pdMS_TO_TICKS(20000)) != pdTRUE) {
        snprintf(out, cap, "timed out\n");
        return ESP_ERR_TIMEOUT;
    }
    snprintf(out, cap, "%s", s_job.out);
    esp_err_t ret = s_job.ret;
    xSemaphoreGive(s_job_lock);
    return ret;
}
