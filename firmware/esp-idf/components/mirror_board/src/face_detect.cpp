/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * The detector is ESP-DL's two-stage MSR+MNP int8 model (espressif/
 * human_face_detect): MSR proposes boxes from a 160x120 copy of the image,
 * MNP re-scores each one from a 48x48 crop. On the P4 that is ~15 ms for the
 * first stage plus ~3 ms per candidate. The models are packed into flash
 * rodata and their parameters copied to PSRAM when loaded; activations are
 * PSRAM too (ESP-DL only uses internal RAM when asked to with
 * max_internal_size, and this does not ask).
 */
#include <algorithm>

#include "esp_heap_caps.h"
#include "esp_log.h"
#include "human_face_detect.hpp"

#include "face_detect.h"

static const char *TAG = "face_detect";

#ifndef FACE_MSR_SCORE_THR
#define FACE_MSR_SCORE_THR 0.3f
#endif

static HumanFaceDetect *s_detect;

extern "C" esp_err_t face_detect_init(void)
{
    if (s_detect) {
        return ESP_OK;
    }
    /* lazy_load = false, so the cost lands here, at start-up, and not inside
     * the first detection's timing. */
    s_detect = new HumanFaceDetect(HumanFaceDetect::MSRMNP_S8_V1, false);
    if (!s_detect) {
        return ESP_ERR_NO_MEM;
    }
    s_detect->set_score_thr(FACE_MSR_SCORE_THR, 0);
    return ESP_OK;
}

extern "C" int face_detect_rgb565(const uint16_t *px, int w, int h, face_box_t *out, int max)
{
    if (!s_detect || !px || w <= 0 || h <= 0 || max <= 0) {
        return 0;
    }
    dl::image::img_t img = {
        .data = (void *)px,
        .width = (uint16_t)w,
        .height = (uint16_t)h,
        .pix_type = dl::image::DL_IMAGE_PIX_TYPE_RGB565LE,
    };
    std::list<dl::detect::result_t> &res = s_detect->run(img);

    int n = 0;
    for (const auto &r : res) {
        if (n >= max || r.box.size() < 4) {
            break;
        }
        out[n++] = (face_box_t){
            .x = r.box[0],
            .y = r.box[1],
            .w = r.box[2] - r.box[0],
            .h = r.box[3] - r.box[1],
            .score = r.score,
        };
    }
    std::sort(out, out + n, [](const face_box_t &a, const face_box_t &b) { return a.score > b.score; });
    return n;
}
