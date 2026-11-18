#pragma once

#include "esp_err.h"

/** Read bench commands ("led 255 0 0", "led off") from the console UART. */
esp_err_t mirror_serial_start(void);
