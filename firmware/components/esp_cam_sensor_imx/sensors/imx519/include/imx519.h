/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Sony IMX519 (Arducam 16MP autofocus module) for the ESP32-P4 esp_cam_sensor
 * framework. Register tables come from the Raspberry Pi kernel driver.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#include "esp_cam_sensor.h"
#include "esp_cam_sensor_types.h"

#ifdef __cplusplus
extern "C" {
#endif

#define IMX519_SENSOR_NAME "IMX519"

#ifndef IMX519_SCCB_ADDR
#define IMX519_SCCB_ADDR   0x1a
#endif

esp_cam_sensor_device_t *imx519_detect(esp_cam_sensor_config_t *config);

size_t imx519_format_count(void);

const esp_cam_sensor_format_t *imx519_format_by_index(size_t index);

const esp_cam_sensor_format_t *imx519_format_by_size(uint16_t width, uint16_t height);

#ifdef __cplusplus
}
#endif
