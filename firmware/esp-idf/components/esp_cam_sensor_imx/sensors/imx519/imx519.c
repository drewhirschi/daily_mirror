/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Sony IMX519 (Arducam 16MP autofocus module) driver for the ESP32-P4
 * esp_cam_sensor framework. 2x2-binned RAW10 readouts on 2 MIPI lanes at a
 * 408 MHz link (816 Mbps/lane). The AK7375 VCM at I2C 0x0c is a separate
 * motor driver.
 *
 * Structure follows the IMX708 driver in this component; the register tables
 * come from the Raspberry Pi kernel driver.
 */
#include <string.h>
#include <inttypes.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>
#include "driver/gpio.h"
#include "esp_err.h"
#include "esp_log.h"
#include "esp_check.h"

#include "esp_cam_sensor.h"
#include "esp_cam_sensor_detect.h"
#include "imx519_settings.h"
#include "imx519.h"

#ifndef portTICK_RATE_MS
#define portTICK_RATE_MS portTICK_PERIOD_MS
#endif
#define delay_ms(ms) vTaskDelay((ms > portTICK_PERIOD_MS ? ms / portTICK_PERIOD_MS : 1))

/* Pixel rate and link, from the kernel driver. */
#define IMX519_PIXEL_RATE          426666667
#define IMX519_MIPI_CSI_LINE_RATE  816000000   /* 408 MHz link, 2 lanes */
#define IMX519_LINESYNC_ENABLE     1

/*
 * Per-mode timing. HTS is the kernel's line length; VTS is *ours*: the kernel
 * tables run 1080p at 60 fps and 720p at 80 fps, which caps exposure at a few
 * milliseconds. These are stills modes, so each VTS below is chosen for 30 fps
 * (VTS = pixel_rate / (HTS * 30)), doubling or nearly tripling the exposure
 * ceiling. The override is written after the mode table.
 */
#define IMX519_HTS_1080P    0x178b   /* 6027 */
#define IMX519_VTS_1080P    2358     /* 426666667 / (6027 * 30) */
#define IMX519_HTS_720P     0x1800   /* 6144 */
#define IMX519_VTS_720P     2314
#define IMX519_HTS_FULLBIN  0x1970   /* 6512 */
#define IMX519_VTS_FULLBIN  0x0888   /* 2184, kernel value, already 30 fps */

#define IMX519_TLINE_NS(hts)  ((uint32_t)(((uint64_t)(hts) * 1000000000ULL) / IMX519_PIXEL_RATE))

static const char *TAG = "imx519";

enum {
    IMX519_FMT_1920x1080_RAW10_30FPS = 0,
    IMX519_FMT_1280x720_RAW10_30FPS,
    IMX519_FMT_2328x1748_RAW10_30FPS,
    IMX519_FMT_MAX,
};

typedef struct {
    uint32_t hts;
    uint32_t vts;
} imx519_mode_timing_t;

static const imx519_mode_timing_t imx519_timing[IMX519_FMT_MAX] = {
    [IMX519_FMT_1920x1080_RAW10_30FPS] = { IMX519_HTS_1080P,   IMX519_VTS_1080P },
    [IMX519_FMT_1280x720_RAW10_30FPS]  = { IMX519_HTS_720P,    IMX519_VTS_720P },
    [IMX519_FMT_2328x1748_RAW10_30FPS] = { IMX519_HTS_FULLBIN, IMX519_VTS_FULLBIN },
};

static const esp_cam_sensor_isp_info_t imx519_isp_info[] = {
    [IMX519_FMT_1920x1080_RAW10_30FPS] = {
        .isp_v1_info = {
            .version = SENSOR_ISP_INFO_VERSION_DEFAULT,
            .pclk = IMX519_PIXEL_RATE,
            .hts = IMX519_HTS_1080P,
            .vts = IMX519_VTS_1080P,
            .exp_def = IMX519_EXPOSURE_DEFAULT,
            .gain_def = IMX519_ANA_GAIN_DEFAULT,
            .tline_ns = IMX519_TLINE_NS(IMX519_HTS_1080P),
            .bayer_type = ESP_CAM_SENSOR_BAYER_RGGB,
        }
    },
    [IMX519_FMT_1280x720_RAW10_30FPS] = {
        .isp_v1_info = {
            .version = SENSOR_ISP_INFO_VERSION_DEFAULT,
            .pclk = IMX519_PIXEL_RATE,
            .hts = IMX519_HTS_720P,
            .vts = IMX519_VTS_720P,
            .exp_def = IMX519_EXPOSURE_DEFAULT,
            .gain_def = IMX519_ANA_GAIN_DEFAULT,
            .tline_ns = IMX519_TLINE_NS(IMX519_HTS_720P),
            .bayer_type = ESP_CAM_SENSOR_BAYER_RGGB,
        }
    },
    [IMX519_FMT_2328x1748_RAW10_30FPS] = {
        .isp_v1_info = {
            .version = SENSOR_ISP_INFO_VERSION_DEFAULT,
            .pclk = IMX519_PIXEL_RATE,
            .hts = IMX519_HTS_FULLBIN,
            .vts = IMX519_VTS_FULLBIN,
            .exp_def = IMX519_EXPOSURE_DEFAULT,
            .gain_def = IMX519_ANA_GAIN_DEFAULT,
            .tline_ns = IMX519_TLINE_NS(IMX519_HTS_FULLBIN),
            .bayer_type = ESP_CAM_SENSOR_BAYER_RGGB,
        }
    },
};

static const esp_cam_sensor_format_t imx519_format_info[] = {
    [IMX519_FMT_1920x1080_RAW10_30FPS] = {
        .name = "MIPI_2lane_24Minput_RAW10_1920x1080_binned_30fps",
        .format = ESP_CAM_SENSOR_PIXFORMAT_RAW10,
        .port = ESP_CAM_SENSOR_MIPI_CSI,
        .xclk = IMX519_INCLK_FREQ_HZ,
        .width = 1920,
        .height = 1080,
        .regs = imx519_mode_1920x1080_regs,
        .regs_size = sizeof(imx519_mode_1920x1080_regs) / sizeof(imx519_mode_1920x1080_regs[0]),
        .fps = 30,
        .isp_info = &imx519_isp_info[IMX519_FMT_1920x1080_RAW10_30FPS],
        .mipi_info = {
            .mipi_clk = IMX519_MIPI_CSI_LINE_RATE,
            .lane_num = 2,
            .line_sync_en = IMX519_LINESYNC_ENABLE,
        },
        .reserved = (void *)&imx519_timing[IMX519_FMT_1920x1080_RAW10_30FPS],
    },
    [IMX519_FMT_1280x720_RAW10_30FPS] = {
        .name = "MIPI_2lane_24Minput_RAW10_1280x720_binned_30fps",
        .format = ESP_CAM_SENSOR_PIXFORMAT_RAW10,
        .port = ESP_CAM_SENSOR_MIPI_CSI,
        .xclk = IMX519_INCLK_FREQ_HZ,
        .width = 1280,
        .height = 720,
        .regs = imx519_mode_1280x720_regs,
        .regs_size = sizeof(imx519_mode_1280x720_regs) / sizeof(imx519_mode_1280x720_regs[0]),
        .fps = 30,
        .isp_info = &imx519_isp_info[IMX519_FMT_1280x720_RAW10_30FPS],
        .mipi_info = {
            .mipi_clk = IMX519_MIPI_CSI_LINE_RATE,
            .lane_num = 2,
            .line_sync_en = IMX519_LINESYNC_ENABLE,
        },
        .reserved = (void *)&imx519_timing[IMX519_FMT_1280x720_RAW10_30FPS],
    },
    [IMX519_FMT_2328x1748_RAW10_30FPS] = {
        .name = "MIPI_2lane_24Minput_RAW10_2328x1748_binned_30fps",
        .format = ESP_CAM_SENSOR_PIXFORMAT_RAW10,
        .port = ESP_CAM_SENSOR_MIPI_CSI,
        .xclk = IMX519_INCLK_FREQ_HZ,
        .width = 2328,
        .height = 1748,
        .regs = imx519_mode_2328x1748_regs,
        .regs_size = sizeof(imx519_mode_2328x1748_regs) / sizeof(imx519_mode_2328x1748_regs[0]),
        .fps = 30,
        .isp_info = &imx519_isp_info[IMX519_FMT_2328x1748_RAW10_30FPS],
        .mipi_info = {
            .mipi_clk = IMX519_MIPI_CSI_LINE_RATE,
            .lane_num = 2,
            .line_sync_en = IMX519_LINESYNC_ENABLE,
        },
        .reserved = (void *)&imx519_timing[IMX519_FMT_2328x1748_RAW10_30FPS],
    },
};

#ifndef CONFIG_CAMERA_IMX519_MIPI_IF_FORMAT_INDEX_DEFAULT
#define CONFIG_CAMERA_IMX519_MIPI_IF_FORMAT_INDEX_DEFAULT 0
#endif
#define IMX519_DEFAULT_FORMAT_INDEX CONFIG_CAMERA_IMX519_MIPI_IF_FORMAT_INDEX_DEFAULT

_Static_assert(IMX519_DEFAULT_FORMAT_INDEX < IMX519_FMT_MAX, "IMX519 default format index out of range");
_Static_assert(IMX519_FMT_MAX == ARRAY_SIZE(imx519_format_info), "format table mismatch");
_Static_assert(IMX519_FMT_MAX == ARRAY_SIZE(imx519_isp_info), "isp_info table mismatch");

size_t imx519_format_count(void)
{
    return ARRAY_SIZE(imx519_format_info);
}

const esp_cam_sensor_format_t *imx519_format_by_index(size_t index)
{
    if (index >= ARRAY_SIZE(imx519_format_info)) {
        return NULL;
    }
    return &imx519_format_info[index];
}

const esp_cam_sensor_format_t *imx519_format_by_size(uint16_t width, uint16_t height)
{
    for (size_t i = 0; i < ARRAY_SIZE(imx519_format_info); i++) {
        if (imx519_format_info[i].width == width && imx519_format_info[i].height == height) {
            return &imx519_format_info[i];
        }
    }
    return NULL;
}

/*
 * Gain menu (milli-units) and the analog gain codes behind it.
 * IMX519 analog gain = 1024 / (1024 - code), code 0..960 -> 1x..16x.
 * Steps of roughly 1/8 stop.
 */
static const uint32_t imx519_total_gain_val_map[] = {
     1000,  1091,  1189,  1296,  1414,  1542,  1681,  1835,
     2000,  2179,  2376,  2592,  2829,  3084,  3368,  3670,
     4000,  4357,  4763,  5198,  5657,  6169,  6737,  7314,
     8000,  8752,  9481, 10343, 11253, 12337, 13474, 14629,
    16000,
};

static const uint16_t imx519_ana_gain_code_map[] = {
       0,   85,  163,  234,  300,  360,  415,  466,
     512,  554,  593,  629,  662,  692,  720,  745,
     768,  789,  809,  827,  843,  858,  872,  884,
     896,  907,  916,  925,  933,  941,  948,  954,
     960,
};

_Static_assert(ARRAY_SIZE(imx519_total_gain_val_map) == ARRAY_SIZE(imx519_ana_gain_code_map), "gain tables");

/* Bayer phase by flip state, indexed (vflip << 1) | hmirror; from the kernel's codes[] order. */
static const esp_cam_sensor_bayer_pattern_t imx519_bayer_by_flip[4] = {
    [0] = ESP_CAM_SENSOR_BAYER_RGGB,
    [1] = ESP_CAM_SENSOR_BAYER_GRBG,
    [2] = ESP_CAM_SENSOR_BAYER_GBRG,
    [3] = ESP_CAM_SENSOR_BAYER_BGGR,
};

typedef struct {
    uint32_t exposure_val;
    uint32_t gain_index;
    uint8_t  hmirror;
    uint8_t  vflip;
    esp_cam_sensor_format_t   format;
    esp_cam_sensor_isp_info_t isp_info;
} imx519_para_t;

/* ------------------------------------------------------------------ */
static esp_err_t imx519_read(esp_sccb_io_handle_t sccb, uint16_t reg, uint8_t *val)
{
    return esp_sccb_transmit_receive_reg_a16v8(sccb, reg, val);
}

static esp_err_t imx519_write(esp_sccb_io_handle_t sccb, uint16_t reg, uint8_t val)
{
    return esp_sccb_transmit_reg_a16v8(sccb, reg, val);
}

static esp_err_t imx519_write16(esp_sccb_io_handle_t sccb, uint16_t reg, uint16_t val)
{
    esp_err_t ret = imx519_write(sccb, reg, (val >> 8) & 0xff);
    if (ret == ESP_OK) {
        ret = imx519_write(sccb, reg + 1, val & 0xff);
    }
    return ret;
}

static esp_err_t imx519_write_array(esp_sccb_io_handle_t sccb, const imx519_reginfo_t *regs)
{
    esp_err_t ret = ESP_OK;
    int i = 0;
    while (ret == ESP_OK && regs[i].reg != IMX519_REG_END) {
        if (regs[i].reg == IMX519_REG_DELAY) {
            delay_ms(regs[i].val);
        } else {
            ret = imx519_write(sccb, regs[i].reg, regs[i].val);
        }
        i++;
    }
    ESP_LOGD(TAG, "wrote %d regs", i);
    return ret;
}

/* ------------------------------------------------------------------ */
static esp_err_t imx519_get_sensor_id(esp_cam_sensor_device_t *dev, esp_cam_sensor_id_t *id)
{
    uint8_t h = 0, l = 0;
    esp_err_t ret = imx519_read(dev->sccb_handle, IMX519_REG_CHIP_ID_H, &h);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "read chip id high failed");
    ret = imx519_read(dev->sccb_handle, IMX519_REG_CHIP_ID_L, &l);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "read chip id low failed");
    id->pid = (h << 8) | l;
    return ESP_OK;
}

static esp_err_t imx519_set_stream(esp_cam_sensor_device_t *dev, int enable)
{
    esp_err_t ret = imx519_write(dev->sccb_handle, IMX519_REG_MODE_SELECT, enable ? 0x01 : 0x00);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "set stream failed");
    dev->stream_status = enable;
    ESP_LOGD(TAG, "stream=%d", enable);
    return ret;
}

static esp_err_t imx519_hw_reset(esp_cam_sensor_device_t *dev)
{
    if (dev->reset_pin >= 0) {
        gpio_set_level(dev->reset_pin, 0);
        delay_ms(10);
        gpio_set_level(dev->reset_pin, 1);
        delay_ms(10);
    }
    return ESP_OK;
}

static esp_err_t imx519_apply_orientation(esp_cam_sensor_device_t *dev)
{
    imx519_para_t *para = (imx519_para_t *)dev->priv;
    uint8_t regval = 0;

    if (para == NULL) {
        return ESP_ERR_INVALID_STATE;
    }
    if (para->hmirror) {
        regval |= IMX519_ORIENTATION_HMIRROR;
    }
    if (para->vflip) {
        regval |= IMX519_ORIENTATION_VFLIP;
    }
    esp_err_t ret = imx519_write(dev->sccb_handle, IMX519_REG_ORIENTATION, regval);
    if (ret != ESP_OK) {
        return ret;
    }
    para->isp_info.isp_v1_info.bayer_type =
        imx519_bayer_by_flip[(para->vflip ? 2 : 0) | (para->hmirror ? 1 : 0)];
    return ESP_OK;
}

static esp_err_t imx519_set_orientation(esp_cam_sensor_device_t *dev, int hmirror, int vflip)
{
    imx519_para_t *para = (imx519_para_t *)dev->priv;
    if (para == NULL) {
        return ESP_ERR_INVALID_STATE;
    }
    para->hmirror = hmirror ? 1 : 0;
    para->vflip = vflip ? 1 : 0;
    if (dev->stream_status) {
        ESP_LOGW(TAG, "flip changed while streaming: the ISP keeps the Bayer phase it latched");
    }
    return imx519_apply_orientation(dev);
}

static uint32_t imx519_vts(esp_cam_sensor_device_t *dev)
{
    if (dev && dev->cur_format && dev->cur_format->isp_info) {
        return dev->cur_format->isp_info->isp_v1_info.vts;
    }
    return IMX519_VTS_1080P;
}

static uint32_t imx519_exposure_max(esp_cam_sensor_device_t *dev)
{
    uint32_t vts = imx519_vts(dev);
    if (vts <= IMX519_EXPOSURE_OFFSET + IMX519_EXPOSURE_MIN) {
        return IMX519_EXPOSURE_MIN;
    }
    return vts - IMX519_EXPOSURE_OFFSET;
}

static uint32_t imx519_tline_ns(esp_cam_sensor_device_t *dev)
{
    if (dev && dev->cur_format && dev->cur_format->isp_info) {
        return dev->cur_format->isp_info->isp_v1_info.tline_ns;
    }
    return IMX519_TLINE_NS(IMX519_HTS_1080P);
}

static uint32_t imx519_clamp_exposure(esp_cam_sensor_device_t *dev, uint32_t lines)
{
    uint32_t max = imx519_exposure_max(dev);
    if (lines < IMX519_EXPOSURE_MIN) {
        lines = IMX519_EXPOSURE_MIN;
    }
    if (lines > max) {
        lines = max;
    }
    return lines;
}

static esp_err_t imx519_set_exposure(esp_cam_sensor_device_t *dev, uint32_t lines)
{
    uint32_t req = lines;
    lines = imx519_clamp_exposure(dev, lines);
    esp_err_t ret = imx519_write16(dev->sccb_handle, IMX519_REG_EXPOSURE_H, (uint16_t)lines);
#if CONFIG_CAMERA_IMX519_DEBUG_LOG
    ESP_LOGI(TAG, "exposure req %" PRIu32 " -> %" PRIu32 " lines (%s)", req, lines, esp_err_to_name(ret));
#else
    (void)req;
#endif
    if (ret == ESP_OK && dev->priv) {
        ((imx519_para_t *)dev->priv)->exposure_val = lines;
    }
    return ret;
}

static esp_err_t imx519_set_analog_gain(esp_cam_sensor_device_t *dev, uint32_t code)
{
    if (code > IMX519_ANA_GAIN_MAX) {
        code = IMX519_ANA_GAIN_MAX;
    }
    return imx519_write16(dev->sccb_handle, IMX519_REG_ANALOG_GAIN_H, (uint16_t)code);
}

static esp_err_t imx519_set_gain_index(esp_cam_sensor_device_t *dev, uint32_t index)
{
    if (index >= ARRAY_SIZE(imx519_ana_gain_code_map)) {
        index = ARRAY_SIZE(imx519_ana_gain_code_map) - 1;
    }
    esp_err_t ret = imx519_write16(dev->sccb_handle, IMX519_REG_ANALOG_GAIN_H,
                                   imx519_ana_gain_code_map[index]);
#if CONFIG_CAMERA_IMX519_DEBUG_LOG
    ESP_LOGI(TAG, "gain index %" PRIu32 " -> code %u (%s)", index, imx519_ana_gain_code_map[index], esp_err_to_name(ret));
#endif
    if (ret == ESP_OK && dev->priv) {
        ((imx519_para_t *)dev->priv)->gain_index = index;
    }
    return ret;
}

static esp_err_t imx519_set_digital_gain(esp_cam_sensor_device_t *dev, uint32_t val)
{
    if (val < IMX519_DGTL_GAIN_MIN) {
        val = IMX519_DGTL_GAIN_MIN;
    }
    if (val > IMX519_DGTL_GAIN_MAX) {
        val = IMX519_DGTL_GAIN_MAX;
    }
    return imx519_write16(dev->sccb_handle, IMX519_REG_DIGITAL_GAIN_H, (uint16_t)val);
}

/* 0 = off, 1 = colour bars, 2 = solid black (all four colour regs zero), 3 = solid mid grey. */
static esp_err_t imx519_set_test_pattern(esp_cam_sensor_device_t *dev, int enable)
{
    esp_err_t ret = ESP_OK;
    if (enable == 2 || enable == 3) {
        uint16_t v = (enable == 3) ? 0x0200 : 0x0000;   /* 10-bit: 512 = mid grey */
        for (uint16_t reg = 0x0602; reg <= 0x0608 && ret == ESP_OK; reg += 2) {
            ret = imx519_write16(dev->sccb_handle, reg, v);
        }
        if (ret == ESP_OK) {
            ret = imx519_write16(dev->sccb_handle, IMX519_REG_TEST_PATTERN_H, 0x0001);
        }
        return ret;
    }
    return imx519_write16(dev->sccb_handle, IMX519_REG_TEST_PATTERN_H,
                          enable ? IMX519_TEST_PATTERN_COLORBARS : IMX519_TEST_PATTERN_DISABLE);
}

static esp_err_t imx519_query_para_desc(esp_cam_sensor_device_t *dev, esp_cam_sensor_param_desc_t *qdesc)
{
    esp_err_t ret = ESP_OK;
    switch (qdesc->id) {
    case ESP_CAM_SENSOR_VFLIP:
    case ESP_CAM_SENSOR_HMIRROR:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_NUMBER;
        qdesc->number.minimum = 0;
        qdesc->number.maximum = 1;
        qdesc->number.step = 1;
        qdesc->default_value = 0;
        break;
    case ESP_CAM_SENSOR_EXPOSURE_VAL:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_NUMBER;
        qdesc->number.minimum = IMX519_EXPOSURE_MIN;
        qdesc->number.maximum = imx519_exposure_max(dev);
        qdesc->number.step = IMX519_EXPOSURE_STEP;
        qdesc->default_value = imx519_clamp_exposure(dev, IMX519_EXPOSURE_DEFAULT);
        break;
    case ESP_CAM_SENSOR_GAIN:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_ENUMERATION;
        qdesc->enumeration.count = ARRAY_SIZE(imx519_total_gain_val_map);
        qdesc->enumeration.elements = imx519_total_gain_val_map;
        qdesc->default_value = 0;
        break;
    case ESP_CAM_SENSOR_ANGAIN:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_NUMBER;
        qdesc->number.minimum = IMX519_ANA_GAIN_MIN;
        qdesc->number.maximum = IMX519_ANA_GAIN_MAX;
        qdesc->number.step = 1;
        qdesc->default_value = IMX519_ANA_GAIN_DEFAULT;
        break;
    case ESP_CAM_SENSOR_DGAIN:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_NUMBER;
        qdesc->number.minimum = IMX519_DGTL_GAIN_MIN;
        qdesc->number.maximum = IMX519_DGTL_GAIN_MAX;
        qdesc->number.step = 1;
        qdesc->default_value = IMX519_DGTL_GAIN_DEFAULT;
        break;
    default:
        ESP_LOGD(TAG, "id=%" PRIx32 " not supported", qdesc->id);
        ret = ESP_ERR_INVALID_ARG;
        break;
    }
    return ret;
}

static esp_err_t imx519_get_para_value(esp_cam_sensor_device_t *dev, uint32_t id, void *arg, size_t size)
{
    imx519_para_t *para = (imx519_para_t *)dev->priv;
    if (para == NULL || arg == NULL || size < sizeof(uint32_t)) {
        return ESP_ERR_INVALID_ARG;
    }
    switch (id) {
    case ESP_CAM_SENSOR_EXPOSURE_VAL:
        *(uint32_t *)arg = para->exposure_val;
        break;
    case ESP_CAM_SENSOR_GAIN:
        *(uint32_t *)arg = para->gain_index;
        break;
    case ESP_CAM_SENSOR_HMIRROR:
        *(uint32_t *)arg = para->hmirror;
        break;
    case ESP_CAM_SENSOR_VFLIP:
        *(uint32_t *)arg = para->vflip;
        break;
    default:
        return ESP_ERR_NOT_SUPPORTED;
    }
    return ESP_OK;
}

static esp_err_t imx519_set_para_value(esp_cam_sensor_device_t *dev, uint32_t id, const void *arg, size_t size)
{
    esp_err_t ret = ESP_OK;
    switch (id) {
    case ESP_CAM_SENSOR_VFLIP: {
        const imx519_para_t *para = (const imx519_para_t *)dev->priv;
        ret = imx519_set_orientation(dev, para ? para->hmirror : 0, *(const int *)arg);
        break;
    }
    case ESP_CAM_SENSOR_HMIRROR: {
        const imx519_para_t *para = (const imx519_para_t *)dev->priv;
        ret = imx519_set_orientation(dev, *(const int *)arg, para ? para->vflip : 0);
        break;
    }
    case ESP_CAM_SENSOR_EXPOSURE_VAL:
        ret = imx519_set_exposure(dev, *(const uint32_t *)arg);
        break;
    case ESP_CAM_SENSOR_EXPOSURE_US: {
        uint32_t us = *(const uint32_t *)arg;
        uint32_t lines = (uint32_t)(((uint64_t)us * 1000) / imx519_tline_ns(dev));
        ret = imx519_set_exposure(dev, lines);
        break;
    }
    case ESP_CAM_SENSOR_GAIN:
        ret = imx519_set_gain_index(dev, *(const uint32_t *)arg);
        break;
    case ESP_CAM_SENSOR_ANGAIN:
        ret = imx519_set_analog_gain(dev, *(const uint32_t *)arg);
        break;
    case ESP_CAM_SENSOR_GROUP_EXP_GAIN: {
        const esp_cam_sensor_gh_exp_gain_t *g = (const esp_cam_sensor_gh_exp_gain_t *)arg;
        uint32_t lines;
        if (g->exposure_val != 0) {
            lines = g->exposure_val;
        } else if (g->exposure_us != 0) {
            lines = (uint32_t)(((uint64_t)g->exposure_us * 1000) / imx519_tline_ns(dev));
        } else {
            ret = ESP_ERR_INVALID_ARG;
            break;
        }
        ret = imx519_set_exposure(dev, lines);
        if (ret == ESP_OK) {
            ret = imx519_set_gain_index(dev, g->gain_index);
        }
        break;
    }
    case ESP_CAM_SENSOR_DGAIN:
        ret = imx519_set_digital_gain(dev, *(const uint32_t *)arg);
        break;
    default:
        ESP_LOGE(TAG, "set id=%" PRIx32 " not supported", id);
        ret = ESP_ERR_INVALID_ARG;
        break;
    }
    return ret;
}

static esp_err_t imx519_query_support_formats(esp_cam_sensor_device_t *dev, esp_cam_sensor_format_array_t *formats)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, formats);
    formats->count = ARRAY_SIZE(imx519_format_info);
    formats->format_array = &imx519_format_info[0];
    return ESP_OK;
}

static esp_err_t imx519_query_support_capability(esp_cam_sensor_device_t *dev, esp_cam_sensor_capability_t *caps)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, caps);
    caps->fmt_raw = 1;
    return ESP_OK;
}

static esp_err_t imx519_select_format(esp_cam_sensor_device_t *dev, const esp_cam_sensor_format_t *format)
{
    imx519_para_t *para = (imx519_para_t *)dev->priv;
    if (para == NULL) {
        dev->cur_format = format;
        return ESP_ERR_INVALID_STATE;
    }
    para->format = *format;
    if (format->isp_info) {
        if (format->isp_info != &para->isp_info) {
            memcpy(&para->isp_info, format->isp_info, sizeof(para->isp_info));
        }
        para->format.isp_info = &para->isp_info;
    }
    dev->cur_format = &para->format;
    return ESP_OK;
}

static esp_err_t imx519_set_format(esp_cam_sensor_device_t *dev, const esp_cam_sensor_format_t *format)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    esp_err_t ret = ESP_OK;

    if (format == NULL) {
        format = &imx519_format_info[IMX519_DEFAULT_FORMAT_INDEX];
    }

    ret = imx519_write_array(dev->sccb_handle, imx519_common_regs);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write common regs failed");

    ret = imx519_write_array(dev->sccb_handle, (const imx519_reginfo_t *)format->regs);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write mode regs failed");

    /* Our frame length, after the mode table's own. */
    const imx519_mode_timing_t *t = (const imx519_mode_timing_t *)format->reserved;
    if (t) {
        ret = imx519_write16(dev->sccb_handle, IMX519_REG_FRAME_LENGTH_H, (uint16_t)t->vts);
        ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write frame length failed");
    }

    /* No reset line on the Pi-style connector, so the sensor keeps register
       state across our reboots: clear the test pattern explicitly. */
    ret = imx519_write16(dev->sccb_handle, IMX519_REG_TEST_PATTERN_H, IMX519_TEST_PATTERN_DISABLE);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "clear test pattern failed");

    /* Start from a known exposure and unity gain, whatever the table left. */
    ret = imx519_write16(dev->sccb_handle, IMX519_REG_EXPOSURE_H, IMX519_EXPOSURE_DEFAULT);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write exposure failed");
    ret = imx519_write16(dev->sccb_handle, IMX519_REG_ANALOG_GAIN_H, IMX519_ANA_GAIN_DEFAULT);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write gain failed");
    ret = imx519_write16(dev->sccb_handle, IMX519_REG_DIGITAL_GAIN_H, IMX519_DGTL_GAIN_DEFAULT);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "write dgain failed");

    ret = imx519_select_format(dev, format);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "no device state to shadow the format into");

    ret = imx519_apply_orientation(dev);
    ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "apply orientation failed");

    imx519_para_t *para = (imx519_para_t *)dev->priv;
    para->exposure_val = IMX519_EXPOSURE_DEFAULT;
    para->gain_index = 0;

    ESP_LOGI(TAG, "set format: %s (vts %" PRIu32 ", max exposure %" PRIu32 " lines = %" PRIu32 " us)",
             format->name, t ? t->vts : 0, imx519_exposure_max(dev),
             (uint32_t)(((uint64_t)imx519_exposure_max(dev) * imx519_tline_ns(dev)) / 1000));
    return ret;
}

static esp_err_t imx519_get_format(esp_cam_sensor_device_t *dev, esp_cam_sensor_format_t *format)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, format);
    if (dev->cur_format == NULL) {
        return ESP_FAIL;
    }
    memcpy(format, dev->cur_format, sizeof(esp_cam_sensor_format_t));
    return ESP_OK;
}

static esp_err_t imx519_priv_ioctl(esp_cam_sensor_device_t *dev, uint32_t cmd, void *arg)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    esp_err_t ret = ESP_OK;
    uint8_t regval = 0;
    esp_cam_sensor_reg_val_t *sensor_reg;

    switch (cmd) {
    case ESP_CAM_SENSOR_IOC_HW_RESET:
        ret = imx519_hw_reset(dev);
        break;
    case ESP_CAM_SENSOR_IOC_S_STREAM:
        ret = imx519_set_stream(dev, *(int *)arg);
        break;
    case ESP_CAM_SENSOR_IOC_S_TEST_PATTERN:
        ret = imx519_set_test_pattern(dev, *(int *)arg);
        break;
    case ESP_CAM_SENSOR_IOC_S_REG:
        sensor_reg = (esp_cam_sensor_reg_val_t *)arg;
        ret = imx519_write(dev->sccb_handle, sensor_reg->regaddr, sensor_reg->value);
        break;
    case ESP_CAM_SENSOR_IOC_G_REG:
        sensor_reg = (esp_cam_sensor_reg_val_t *)arg;
        ret = imx519_read(dev->sccb_handle, sensor_reg->regaddr, &regval);
        if (ret == ESP_OK) {
            sensor_reg->value = regval;
        }
        break;
    case ESP_CAM_SENSOR_IOC_G_CHIP_ID:
        ret = imx519_get_sensor_id(dev, (esp_cam_sensor_id_t *)arg);
        break;
    default:
        ret = ESP_ERR_INVALID_ARG;
        break;
    }
    return ret;
}

static esp_err_t imx519_power_on(esp_cam_sensor_device_t *dev)
{
    esp_err_t ret = ESP_OK;

    if (dev->pwdn_pin >= 0) {
        gpio_config_t conf = { 0 };
        conf.pin_bit_mask = 1LL << dev->pwdn_pin;
        conf.mode = GPIO_MODE_OUTPUT;
        ret = gpio_config(&conf);
        ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "pwdn pin config failed");
        gpio_set_level(dev->pwdn_pin, 1);
        delay_ms(10);
    }
    if (dev->reset_pin >= 0) {
        gpio_config_t conf = { 0 };
        conf.pin_bit_mask = 1LL << dev->reset_pin;
        conf.mode = GPIO_MODE_OUTPUT;
        ret = gpio_config(&conf);
        ESP_RETURN_ON_FALSE(ret == ESP_OK, ret, TAG, "reset pin config failed");
        gpio_set_level(dev->reset_pin, 0);
        delay_ms(10);
        gpio_set_level(dev->reset_pin, 1);
        delay_ms(10);
    }
    return ret;
}

static esp_err_t imx519_power_off(esp_cam_sensor_device_t *dev)
{
    if (dev->reset_pin >= 0) {
        gpio_set_level(dev->reset_pin, 0);
    }
    if (dev->pwdn_pin >= 0) {
        gpio_set_level(dev->pwdn_pin, 0);
    }
    return ESP_OK;
}

static esp_err_t imx519_delete(esp_cam_sensor_device_t *dev)
{
    ESP_LOGD(TAG, "del imx519 (%p)", dev);
    if (dev) {
        free(dev);
    }
    return ESP_OK;
}

static const esp_cam_sensor_ops_t imx519_ops = {
    .query_para_desc = imx519_query_para_desc,
    .get_para_value = imx519_get_para_value,
    .set_para_value = imx519_set_para_value,
    .query_support_formats = imx519_query_support_formats,
    .query_support_capability = imx519_query_support_capability,
    .set_format = imx519_set_format,
    .get_format = imx519_get_format,
    .priv_ioctl = imx519_priv_ioctl,
    .del = imx519_delete,
};

esp_cam_sensor_device_t *imx519_detect(esp_cam_sensor_config_t *config)
{
    if (config == NULL) {
        return NULL;
    }

    esp_cam_sensor_device_t *dev = calloc(1, sizeof(esp_cam_sensor_device_t) + sizeof(imx519_para_t));
    if (dev == NULL) {
        ESP_LOGE(TAG, "no memory for camera device");
        return NULL;
    }
    dev->priv = (uint8_t *)dev + sizeof(esp_cam_sensor_device_t);

    dev->name = (char *)IMX519_SENSOR_NAME;
    dev->sccb_handle = config->sccb_handle;
    dev->xclk_pin = config->xclk_pin;
    dev->reset_pin = config->reset_pin;
    dev->pwdn_pin = config->pwdn_pin;
    dev->sensor_port = config->sensor_port;
    dev->ops = &imx519_ops;
    imx519_select_format(dev, &imx519_format_info[IMX519_DEFAULT_FORMAT_INDEX]);

    if (config->sensor_port != ESP_CAM_SENSOR_MIPI_CSI) {
        ESP_LOGE(TAG, "only MIPI-CSI is supported");
        goto err;
    }
    if (imx519_power_on(dev) != ESP_OK) {
        ESP_LOGE(TAG, "power on failed");
        goto err;
    }
    if (imx519_get_sensor_id(dev, &dev->id) != ESP_OK) {
        ESP_LOGE(TAG, "get chip id failed");
        goto err;
    }
    if (dev->id.pid != IMX519_CHIP_ID) {
        ESP_LOGE(TAG, "sensor is not IMX519, PID=0x%04x", dev->id.pid);
        goto err;
    }
    ESP_LOGI(TAG, "detected IMX519, PID=0x%04x", dev->id.pid);
    return dev;

err:
    imx519_power_off(dev);
    free(dev);
    return NULL;
}

#if CONFIG_CAMERA_IMX519_AUTO_DETECT_MIPI_INTERFACE_SENSOR
ESP_CAM_SENSOR_DETECT_FN(imx519_detect, ESP_CAM_SENSOR_MIPI_CSI, IMX519_SCCB_ADDR)
{
    ((esp_cam_sensor_config_t *)config)->sensor_port = ESP_CAM_SENSOR_MIPI_CSI;
    return imx519_detect(config);
}
#endif
