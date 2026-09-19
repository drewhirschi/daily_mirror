/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * IMX519 register map (the subset this driver touches). Values match
 * raspberrypi/linux drivers/media/i2c/imx519.c.
 */
#pragma once

#define IMX519_REG_END              0xffff
#define IMX519_REG_DELAY            0xfffe

#define IMX519_REG_CHIP_ID_H        0x0016
#define IMX519_REG_CHIP_ID_L        0x0017
#define IMX519_CHIP_ID              0x0519

#define IMX519_REG_MODE_SELECT      0x0100  /*!< 0=standby, 1=streaming */
#define IMX519_REG_ORIENTATION      0x0101  /*!< bit0 = h flip, bit1 = v flip */
#define IMX519_ORIENTATION_HMIRROR  0x01
#define IMX519_ORIENTATION_VFLIP    0x02

#define IMX519_REG_FRAME_LENGTH_H   0x0340  /*!< VTS */
#define IMX519_REG_FRAME_LENGTH_L   0x0341
#define IMX519_FRAME_LENGTH_MAX     0xffdc

#define IMX519_REG_EXPOSURE_H       0x0202
#define IMX519_REG_EXPOSURE_L       0x0203
#define IMX519_EXPOSURE_OFFSET      32
#define IMX519_EXPOSURE_MIN         20
#define IMX519_EXPOSURE_STEP        1
#define IMX519_EXPOSURE_DEFAULT     0x03e8

#define IMX519_REG_ANALOG_GAIN_H    0x0204
#define IMX519_REG_ANALOG_GAIN_L    0x0205
#define IMX519_ANA_GAIN_MIN         0       /*!< gain = 1024/(1024-code) */
#define IMX519_ANA_GAIN_MAX         960
#define IMX519_ANA_GAIN_DEFAULT     0

#define IMX519_REG_DIGITAL_GAIN_H   0x020e
#define IMX519_REG_DIGITAL_GAIN_L   0x020f
#define IMX519_DGTL_GAIN_MIN        0x0100
#define IMX519_DGTL_GAIN_MAX        0xffff
#define IMX519_DGTL_GAIN_DEFAULT    0x0100

#define IMX519_REG_TEST_PATTERN_H   0x0600
#define IMX519_REG_TEST_PATTERN_L   0x0601
#define IMX519_TEST_PATTERN_DISABLE 0x0000
#define IMX519_TEST_PATTERN_COLORBARS 0x0002

#define IMX519_INCLK_FREQ_HZ        24000000
