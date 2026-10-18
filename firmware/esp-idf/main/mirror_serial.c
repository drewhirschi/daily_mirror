#include "mirror_serial.h"
#include "mirror_ring.h"

#include <stdio.h>
#include <string.h>
#include "driver/uart.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "mirror_serial";

/*
 * Line commands on the console UART, for bench checks over the USB cable when
 * the board is not on a network:
 *
 *   led 255 0 0     hold the status LED at a colour (0..255 each)
 *   led off         hand the LED back to its patterns
 */
static void run(char *line)
{
    int r, g, b;
    if (sscanf(line, "led %d %d %d", &r, &g, &b) == 3) {
        mirror_ring_test_color(r, g, b);
        ESP_LOGI(TAG, "led %d %d %d", r, g, b);
    } else if (strncmp(line, "led off", 7) == 0) {
        mirror_ring_test_color(-1, -1, -1);
        ESP_LOGI(TAG, "led test off");
    } else if (line[0]) {
        ESP_LOGW(TAG, "unknown command: %s", line);
    }
}

static void serial_task(void *arg)
{
    (void)arg;
    char line[64];
    size_t n = 0;
    uint8_t c;
    for (;;) {
        if (uart_read_bytes(CONFIG_ESP_CONSOLE_UART_NUM, &c, 1, portMAX_DELAY) != 1) {
            continue;
        }
        if (c == '\r' || c == '\n') {
            line[n] = 0;
            run(line);
            n = 0;
        } else if (n < sizeof(line) - 1) {
            line[n++] = (char)c;
        }
    }
}

esp_err_t mirror_serial_start(void)
{
    /* RX only: logging keeps writing to the UART the way it always has. */
    esp_err_t err = uart_driver_install(CONFIG_ESP_CONSOLE_UART_NUM, 256, 0, 0, NULL, 0);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "uart_driver_install failed: %s", esp_err_to_name(err));
        return err;
    }
    return xTaskCreate(serial_task, "serial_cmd", 3072, NULL, 2, NULL) == pdPASS ? ESP_OK : ESP_FAIL;
}
