/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Asahi Kasei AK7375 VCM autofocus actuator - the lens driver on the Arducam
 * 16MP IMX519 module (the Pi overlay binds "asahi-kasei,ak7375" at I2C 0x0c).
 *
 * Register map (8-bit address, from the Linux ak7375 driver):
 *   0x00/0x01  position, 12-bit code left-aligned in a 16-bit word (code << 4)
 *   0x02       control: 0x00 active, 0x40 standby
 * Nothing is readable; presence is an ACK.
 */
#include <string.h>
#include <inttypes.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>

#include "driver/gpio.h"
#include "esp_err.h"
#include "esp_check.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "esp_sccb_intf.h"
#include "esp_cam_motor.h"
#include "esp_cam_motor_detect.h"
#include "ak7375.h"

static const char *TAG = "ak7375";

#define AK7375_REG_POSITION     0x00
#define AK7375_REG_CONT         0x02
#define AK7375_MODE_ACTIVE      0x00
#define AK7375_MODE_STANDBY     0x40
#define AK7375_POWER_DELAY_MS   10

#define AK7375_MIN_POS          0
#define AK7375_MAX_POS          AK7375_MAX_DAC_CODE

#ifndef CONFIG_CAM_MOTOR_AK7375_INIT_POS
#define CONFIG_CAM_MOTOR_AK7375_INIT_POS 2048
#endif
#ifndef CONFIG_CAM_MOTOR_AK7375_PERIOD_US
#define CONFIG_CAM_MOTOR_AK7375_PERIOD_US 1000
#endif
#define AK7375_INIT_POS         CONFIG_CAM_MOTOR_AK7375_INIT_POS
#define AK7375_PERIOD_IN_US     CONFIG_CAM_MOTOR_AK7375_PERIOD_US

/* Step size for the retract ramp on standby, in codes. */
#define AK7375_RETRACT_STEP     64

static esp_err_t ak7375_write_ctrl(esp_sccb_io_handle_t sccb, uint8_t mode)
{
    return esp_sccb_transmit_reg_a8v8(sccb, AK7375_REG_CONT, mode);
}

/* 3-byte transfer {0x00, hi, lo} with hi/lo = (code << 4) big-endian. */
static esp_err_t ak7375_write_pos(esp_sccb_io_handle_t sccb, uint16_t code)
{
    return esp_sccb_transmit_reg_a8v16(sccb, AK7375_REG_POSITION, (uint16_t)((code & 0x0fff) << 4));
}

static esp_err_t ak7375_set_pos_code(esp_cam_motor_device_t *dev, int pos)
{
    if (pos > AK7375_MAX_POS) {
        pos = AK7375_MAX_POS;
    } else if (pos < AK7375_MIN_POS) {
        pos = AK7375_MIN_POS;
    }
    esp_err_t ret = ak7375_write_pos(dev->sccb_handle, (uint16_t)pos);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "failed to write DAC code %d", pos);
        return ESP_CAM_MOTOR_ERR_FAILED_SET_POS;
    }
    dev->current_position = pos;
    dev->moving_start_time = esp_timer_get_time();
    return ESP_OK;
}

static esp_err_t ak7375_active(esp_cam_motor_device_t *dev)
{
    esp_err_t ret = ak7375_write_ctrl(dev->sccb_handle, AK7375_MODE_ACTIVE);
    ESP_RETURN_ON_ERROR(ret, TAG, "failed to leave standby");
    vTaskDelay(pdMS_TO_TICKS(AK7375_POWER_DELAY_MS));
    return ak7375_set_pos_code(dev, dev->current_position);
}

static esp_err_t ak7375_standby(esp_cam_motor_device_t *dev)
{
    int pos = dev->current_position;
    while (pos > AK7375_RETRACT_STEP) {
        pos -= AK7375_RETRACT_STEP;
        if (ak7375_set_pos_code(dev, pos) != ESP_OK) {
            break;
        }
        vTaskDelay(1);
    }
    return ak7375_write_ctrl(dev->sccb_handle, AK7375_MODE_STANDBY);
}

static const esp_cam_motor_format_t ak7375_format_info[] = {
    {
        .name = "DIRECT_mode",
        .mode = ESP_CAM_MOTOR_DIRECT_MODE,
        .step_period = {
            .period_in_us = AK7375_PERIOD_IN_US,
            .codes_per_step = 1,
        },
        .init_position = AK7375_INIT_POS,
        .regs = NULL,
        .regs_size = 0,
        .reserved = NULL,
    },
};

static esp_err_t ak7375_query_para_desc(esp_cam_motor_device_t *dev, esp_cam_motor_param_desc_t *qdesc)
{
    esp_err_t ret = ESP_OK;
    switch (qdesc->id) {
    case ESP_CAM_MOTOR_POSITION_CODE:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_NUMBER;
        qdesc->number.minimum = AK7375_MIN_POS;
        qdesc->number.maximum = AK7375_MAX_POS;
        qdesc->number.step = 1;
        qdesc->default_value = AK7375_INIT_POS;
        break;
    case ESP_CAM_MOTOR_MOVING_START_TIME:
        qdesc->type = ESP_CAM_SENSOR_PARAM_TYPE_U8;
        qdesc->u8.size = sizeof(int64_t);
        break;
    default:
        ESP_LOGD(TAG, "id=%" PRIx32 " is not supported", qdesc->id);
        ret = ESP_ERR_INVALID_ARG;
        break;
    }
    return ret;
}

static esp_err_t ak7375_get_para_value(esp_cam_motor_device_t *dev, uint32_t id, void *arg, size_t size)
{
    esp_err_t ret = ESP_OK;
    switch (id) {
    case ESP_CAM_MOTOR_POSITION_CODE:
        ESP_RETURN_ON_FALSE(arg && size >= sizeof(int32_t), ESP_ERR_INVALID_ARG, TAG, "para size err");
        *(int32_t *)arg = dev->current_position;
        break;
    case ESP_CAM_MOTOR_MOVING_START_TIME:
        ESP_RETURN_ON_FALSE(arg && size == sizeof(int64_t), ESP_ERR_INVALID_ARG, TAG, "para size err");
        *(int64_t *)arg = dev->moving_start_time;
        break;
    default:
        ret = ESP_ERR_NOT_SUPPORTED;
        break;
    }
    return ret;
}

static esp_err_t ak7375_set_para_value(esp_cam_motor_device_t *dev, uint32_t id, const void *arg, size_t size)
{
    esp_err_t ret = ESP_OK;
    switch (id) {
    case ESP_CAM_MOTOR_POSITION_CODE:
        ESP_RETURN_ON_FALSE(arg && size >= sizeof(int32_t), ESP_ERR_INVALID_ARG, TAG, "para size err");
        ret = ak7375_set_pos_code(dev, *(const int32_t *)arg);
        break;
    default:
        ESP_LOGE(TAG, "set id=%" PRIx32 " is not supported", id);
        ret = ESP_ERR_INVALID_ARG;
        break;
    }
    return ret;
}

static esp_err_t ak7375_query_support_formats(esp_cam_motor_device_t *dev, esp_cam_motor_fmt_array_t *formats)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, formats);
    formats->count = ARRAY_SIZE(ak7375_format_info);
    formats->fmt_array = &ak7375_format_info[0];
    return ESP_OK;
}

static esp_err_t ak7375_set_format(esp_cam_motor_device_t *dev, const esp_cam_motor_format_t *format)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    if (format == NULL) {
        format = &ak7375_format_info[0];
    }
    dev->cur_format = format;
    if (ak7375_set_pos_code(dev, format->init_position) != ESP_OK) {
        ESP_LOGE(TAG, "failed to move to init position %d", format->init_position);
        return ESP_CAM_MOTOR_ERR_FAILED_SET_FORMAT;
    }
    return ESP_OK;
}

static esp_err_t ak7375_get_format(esp_cam_motor_device_t *dev, esp_cam_motor_format_t *format)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, format);
    if (dev->cur_format == NULL) {
        return ESP_FAIL;
    }
    memcpy(format, dev->cur_format, sizeof(esp_cam_motor_format_t));
    return ESP_OK;
}

static esp_err_t ak7375_hw_power_on(esp_cam_motor_device_t *dev, bool en)
{
    if (dev->pwdn_pin >= 0) {
        gpio_config_t conf = {
            .pin_bit_mask = 1ULL << dev->pwdn_pin,
            .mode = GPIO_MODE_OUTPUT,
        };
        gpio_config(&conf);
        gpio_set_level(dev->pwdn_pin, en ? 1 : 0);
        vTaskDelay(pdMS_TO_TICKS(AK7375_POWER_DELAY_MS));
    }
    return ESP_OK;
}

static esp_err_t ak7375_priv_ioctl(esp_cam_motor_device_t *dev, uint32_t cmd, void *arg)
{
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, dev);
    ESP_CAM_SENSOR_NULL_POINTER_CHECK(TAG, arg);
    switch (cmd) {
    case ESP_CAM_MOTOR_IOC_HW_POWER_ON:
        return ak7375_hw_power_on(dev, *(int *)arg);
    case ESP_CAM_MOTOR_IOC_SW_STANDBY:
        return *(int *)arg ? ak7375_standby(dev) : ak7375_active(dev);
    case ESP_CAM_MOTOR_IOC_S_REG: {
        esp_cam_motor_reg_val_t *rv = (esp_cam_motor_reg_val_t *)arg;
        return esp_sccb_transmit_reg_a8v8(dev->sccb_handle, (uint8_t)rv->regaddr, (uint8_t)rv->value);
    }
    default:
        return ESP_ERR_INVALID_ARG;
    }
}

static esp_err_t ak7375_delete(esp_cam_motor_device_t *dev)
{
    if (dev) {
        free(dev);
    }
    return ESP_OK;
}

static const esp_cam_motor_ops_t ak7375_ops = {
    .query_para_desc = ak7375_query_para_desc,
    .get_para_value = ak7375_get_para_value,
    .set_para_value = ak7375_set_para_value,
    .query_support_formats = ak7375_query_support_formats,
    .set_format = ak7375_set_format,
    .get_format = ak7375_get_format,
    .priv_ioctl = ak7375_priv_ioctl,
    .del = ak7375_delete,
};

esp_cam_motor_device_t *ak7375_detect(esp_cam_motor_config_t *config)
{
    if (config == NULL) {
        return NULL;
    }
    esp_cam_motor_device_t *dev = calloc(1, sizeof(esp_cam_motor_device_t));
    if (dev == NULL) {
        ESP_LOGE(TAG, "no memory for motor");
        return NULL;
    }
    dev->name = (char *)TAG;
    dev->sccb_handle = config->sccb_handle;
    dev->reset_pin = config->reset_pin;
    dev->pwdn_pin = config->pwdn_pin;
    dev->signal_pin = config->signal_pin;
    dev->ops = &ak7375_ops;

    ak7375_hw_power_on(dev, true);

    /* Presence = an ACK on the control register; also wakes the coil driver. */
    if (ak7375_write_ctrl(dev->sccb_handle, AK7375_MODE_ACTIVE) != ESP_OK) {
        ESP_LOGD(TAG, "no device answered at 0x%02x", AK7375_SCCB_ADDR);
        goto err;
    }
    vTaskDelay(pdMS_TO_TICKS(AK7375_POWER_DELAY_MS));

    if (ak7375_set_format(dev, NULL) != ESP_OK) {
        ESP_LOGE(TAG, "failed to set default format");
        goto err;
    }
    ESP_LOGI(TAG, "detected AK7375 VCM at 0x%02x, lens parked at code %d (range %d..%d)",
             AK7375_SCCB_ADDR, dev->current_position, AK7375_MIN_POS, AK7375_MAX_POS);
    return dev;

err:
    ak7375_hw_power_on(dev, false);
    free(dev);
    return NULL;
}

#if CONFIG_CAM_MOTOR_AK7375_AUTO_DETECT
ESP_CAM_MOTOR_DETECT_FN(ak7375_detect, NULL, AK7375_SCCB_ADDR)
{
    return ak7375_detect(config);
}
#endif
