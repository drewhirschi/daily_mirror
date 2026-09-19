#include <string.h>

#include "mirror_press.h"
#include "mirror_ring.h"
#include "mirror_upload.h"
#include "mirror_board.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "driver/gpio.h"

static const char *TAG = "mirror_press";

/* Same numbers as the Pi (device/src/main.rs): a 60 ms settle is long enough
 * for these switches and short enough that the press still feels immediate,
 * and a 20 ms poll costs nothing next to it. */
#define DEBOUNCE_MS 60
#define POLL_MS     20

static uint8_t *s_last_jpg;         /* the committed photo, in PSRAM */
static size_t s_last_len;
static SemaphoreHandle_t s_last_lock;
static volatile bool s_busy;
static volatile uint32_t s_count;

bool mirror_press_busy(void)     { return s_busy; }
uint32_t mirror_press_count(void) { return s_count; }

bool mirror_press_last_acquire(const uint8_t **jpeg, size_t *len)
{
    if (!s_last_lock) {
        return false;
    }
    xSemaphoreTake(s_last_lock, portMAX_DELAY);
    if (!s_last_jpg || !s_last_len) {
        xSemaphoreGive(s_last_lock);
        return false;
    }
    *jpeg = s_last_jpg;
    *len = s_last_len;
    return true;
}

void mirror_press_last_release(void)
{
    if (s_last_lock) {
        xSemaphoreGive(s_last_lock);
    }
}

/* Copy into the slot. This is the commit point: once it returns true the
 * photo survives the camera buffer going back to the driver, and a later
 * upload failure cannot lose it. */
static bool commit(const uint8_t *jpeg, size_t len)
{
    uint8_t *copy = heap_caps_malloc(len, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!copy) {
        ESP_LOGE(TAG, "no PSRAM for a %u byte photo", (unsigned)len);
        return false;
    }
    memcpy(copy, jpeg, len);
    xSemaphoreTake(s_last_lock, portMAX_DELAY);
    free(s_last_jpg);
    s_last_jpg = copy;
    s_last_len = len;
    xSemaphoreGive(s_last_lock);
    return true;
}

void mirror_press_run(void)
{
    s_busy = true;

    mirror_ring_play(RING_AMBER_SLOW_PULSE, RING_AMBER_RAPID_PULSE);

    uint8_t *jpeg = NULL;
    size_t len = 0;
    esp_err_t err = board_camera_capture(true, &jpeg, &len);
    if (err != ESP_OK || !jpeg || !len) {
        ESP_LOGE(TAG, "capture failed: %s", esp_err_to_name(err));
        board_camera_release(jpeg);
        mirror_ring_play(RING_RED_TRIPLE_PULSE, RING_SOLID_WHITE);
        s_busy = false;
        return;
    }

    bool committed = commit(jpeg, len);
    /* Hand the buffer back before anything slow: on the S3 it is a driver
     * frame buffer, and the pipeline only has two. */
    board_camera_release(jpeg);
    jpeg = NULL;

    if (!committed) {
        mirror_ring_play(RING_RED_TRIPLE_PULSE, RING_SOLID_WHITE);
        s_busy = false;
        return;
    }

    uint32_t w = 0, h = 0;
    board_camera_last_size(&w, &h);
    ESP_LOGI(TAG, "captured %ux%u, %u bytes -> /last.jpg",
             (unsigned)w, (unsigned)h, (unsigned)len);
    s_count++;
    mirror_ring_play(RING_GREEN_FLASH, RING_SOLID_WHITE);

    if (mirror_upload_configured()) {
        mirror_ring_set(RING_BLUE_SLOW_PULSE);
        const uint8_t *body = NULL;
        size_t body_len = 0;
        esp_err_t uerr = ESP_FAIL;
        if (mirror_press_last_acquire(&body, &body_len)) {
            uerr = mirror_upload_jpeg(body, body_len);
            mirror_press_last_release();
        }
        if (uerr != ESP_OK) {
            ESP_LOGW(TAG, "upload failed: %s", mirror_upload_last_result());
            mirror_ring_play(RING_RED_TRIPLE_PULSE, RING_SOLID_WHITE);
        }
    } else {
        ESP_LOGI(TAG, "no server_url/upload_token set - photo kept at /last.jpg");
    }

    mirror_ring_set(RING_SOLID_WHITE);
    s_busy = false;
}

static void button_task(void *arg)
{
    const int pin = board_button_gpio();
    gpio_config_t io = {
        .pin_bit_mask = 1ULL << pin,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    gpio_config(&io);
    ESP_LOGI(TAG, "button on GPIO %d (active low, internal pull-up)", pin);

    for (;;) {
        if (gpio_get_level(pin) == 0) {
            vTaskDelay(pdMS_TO_TICKS(DEBOUNCE_MS));
            if (gpio_get_level(pin) == 0 && !s_busy) {
                mirror_press_run();
                /* Wait for the release, then debounce that too, so one long
                 * hold is one photo rather than a burst. */
                while (gpio_get_level(pin) == 0) {
                    vTaskDelay(pdMS_TO_TICKS(POLL_MS));
                }
                vTaskDelay(pdMS_TO_TICKS(DEBOUNCE_MS));
            }
        }
        vTaskDelay(pdMS_TO_TICKS(POLL_MS));
    }
}

esp_err_t mirror_press_start(void)
{
    s_last_lock = xSemaphoreCreateMutex();
    if (!s_last_lock) {
        return ESP_ERR_NO_MEM;
    }
    if (board_button_gpio() < 0) {
        ESP_LOGW(TAG, "no button GPIO configured - POST /press only");
        return ESP_OK;
    }
    /* 8 KB: the upload runs on this task, and mbedTLS' handshake is most of
     * it. Trimming this is how a press turns into a stack-overflow panic. */
    return xTaskCreate(button_task, "button", 8192, NULL, 5, NULL) == pdPASS
        ? ESP_OK : ESP_ERR_NO_MEM;
}
