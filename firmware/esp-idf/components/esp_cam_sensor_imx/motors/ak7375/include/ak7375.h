/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Asahi Kasei AK7375 voice-coil-motor autofocus actuator (Arducam IMX519
 * module), I2C 0x0c, 12-bit DAC, write-only.
 */
#pragma once

#include "esp_cam_motor.h"

#ifdef __cplusplus
extern "C" {
#endif

#define AK7375_SCCB_ADDR        0x0c
#define AK7375_MAX_DAC_CODE     4095

esp_cam_motor_device_t *ak7375_detect(esp_cam_motor_config_t *config);

#ifdef __cplusplus
}
#endif
