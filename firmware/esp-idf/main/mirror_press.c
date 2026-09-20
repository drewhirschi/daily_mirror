#include <string.h>

#include "mirror_press.h"
#include "mirror_pair.h"
#include "mirror_ring.h"
#include "mirror_upload.h"
#include "mirror_spool.h"
#include "mirror_board.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include <inttypes.h>
#include "driver/gpio.h"

static const char *TAG = "mirror_press";

/* Same numbers as the Pi (device/src/main.rs): a 60 ms settle is long enough
 * for these switches and short enough that the press still feels immediate,
 * and a 20 ms poll costs nothing next to it. */
#define DEBOUNCE_MS 60
#define POLL_MS     20

/* The gesture thresholds, from crates/mirror-core/src/lib.rs `timing`. The
 * detector below is a port of crates/mirror-core/src/button.rs, so the same
 * hold feels the same on the Pi, in the host tests and here. */
#define HOLD_ARM_MS           2000   /* the ring starts filling amber */
#define HOLD_PAIR_MS          5000   /* enter pairing */
#define HOLD_RESET_MS        20000   /* the full reset becomes armed */
#define RESET_CLICK_WINDOW_MS 3000   /* clicks must land inside this */
#define RESET_CLICK_COUNT        3

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

/* Sleep until `ms` after `started`, or return at once if that is already past. */
static void wait_until(int64_t started, uint32_t ms)
{
    int64_t elapsed = (esp_timer_get_time() - started) / 1000;
    if (elapsed < (int64_t)ms) {
        vTaskDelay(pdMS_TO_TICKS((uint32_t)(ms - elapsed)));
    }
}

static esp_timer_handle_t s_flash_timer;

static void flash_on_cb(void *arg)
{
    (void)arg;
    board_flash_set(true);
}

/* Turn the flash on `ms` into the countdown, whatever this task is doing. */
static void arm_flash_at(uint32_t ms)
{
    if (!s_flash_timer) {
        const esp_timer_create_args_t args = {
            .callback = flash_on_cb, .name = "flash_on",
        };
        if (esp_timer_create(&args, &s_flash_timer) != ESP_OK) {
            return;
        }
    }
    esp_timer_stop(s_flash_timer);
    esp_timer_start_once(s_flash_timer, (uint64_t)ms * 1000);
}

static uint32_t flash_lead_ms(void)
{
    if (!board_flash_available()) {
        return 0;
    }
    uint32_t lead = CONFIG_MIRROR_FLASH_LEAD_MS;
    /* Never longer than the countdown it has to fit inside. */
    return lead > MIRROR_RING_COUNTDOWN_MS - 300 ? MIRROR_RING_COUNTDOWN_MS - 300 : lead;
}

/*
 * The countdown, and the photograph at the end of it.
 *
 * The camera work runs *during* the blinks, not after them: focus and the
 * frame flush cost a couple of seconds on this sensor, and doing them once
 * the countdown had finished is what put three seconds of rapid amber between
 * "3, 2, 1" and the actual photo. board_camera_prepare() is given the
 * countdown as its budget and gives up rather than overrun it; the shutter
 * then fires as the last blink ends, marked with one white flash.
 *
 * The ring is driven through RING_LAYER_CAPTURE, which outranks the spool's
 * upload pulse - a press interrupts an upload's feedback immediately and the
 * upload's pattern comes back on its own afterwards. Nothing here waits on
 * the uploader: the spool's lock is held only while a file is written, never
 * across an upload.
 */
void mirror_press_run(mirror_trigger_t trigger)
{
    s_busy = true;

    const int64_t started = esp_timer_get_time();
    mirror_ring_layer_set(RING_LAYER_CAPTURE, RING_AMBER_SLOW_PULSE);

    /*
     * The flash comes on for the last MIRROR_FLASH_LEAD_MS of the countdown
     * and stays on through the grab. The order below is the whole point:
     * focus, then light, then the frame flush that settles auto-exposure, then
     * the photograph. Flushing before the light would meter the dark room and
     * blow the frame out; pulsing the light at the shutter instead of holding
     * it would light only part of a rolling-shutter readout. Both are written
     * up in docs/burst-processing-experiment.md.
     */
    const uint32_t lead = flash_lead_ms();

    /*
     * The light goes on from a timer, not from this task's place in the
     * sequence. Hanging it off "after focus returns" made the two fight for
     * the same seconds: the focus pass needs about 2.6 s on this sensor, and
     * cutting it to fit the lead in front of it left autofocus reporting
     * `searching` on every frame. On a timer the lead is exactly the lead, the
     * focus pass gets the countdown it needs, and its tail runs under the
     * light - which is what we wanted anyway.
     */
    if (lead) {
        arm_flash_at(MIRROR_RING_COUNTDOWN_MS - lead);
    }

    /* Leave the last stretch of the countdown clear, so a focus pass that
     * runs long does not push the shutter past the final blink. */
    board_camera_prepare_focus(MIRROR_RING_COUNTDOWN_MS - 800);

    /* Whatever is left of the countdown goes on settling auto-exposure, with
     * the light already on. */
    int64_t left = (int64_t)MIRROR_RING_COUNTDOWN_MS
                 - (esp_timer_get_time() - started) / 1000;
    board_camera_prepare_settle(left > 0 ? (uint32_t)left : 0);

    wait_until(started, MIRROR_RING_COUNTDOWN_MS);
    mirror_ring_layer_set(RING_LAYER_CAPTURE, RING_WHITE_FLASH);

    uint8_t *jpeg = NULL;
    size_t len = 0;
    esp_err_t err = board_camera_capture(false, &jpeg, &len);
    /* Off the moment the frame is in hand, on every path out of here. The
     * board layer's timeout is the backstop, not the plan. */
    if (s_flash_timer) {
        esp_timer_stop(s_flash_timer);   /* in case the capture beat it */
    }
    board_flash_set(false);
    ESP_LOGI(TAG, "shutter at %" PRId64 " ms after the press (flash %s)",
             (esp_timer_get_time() - started) / 1000, lead ? "on" : "off");
    if (err != ESP_OK || !jpeg || !len) {
        ESP_LOGE(TAG, "capture failed: %s", esp_err_to_name(err));
        board_camera_release(jpeg);
        mirror_ring_layer_play(RING_LAYER_CAPTURE, RING_RED_TRIPLE_PULSE);
        s_busy = false;
        return;
    }

    bool committed = commit(jpeg, len);
    /* Hand the buffer back before anything slow: on the S3 it is a driver
     * frame buffer, and the pipeline only has two. */
    board_camera_release(jpeg);
    jpeg = NULL;

    if (!committed) {
        mirror_ring_layer_play(RING_LAYER_CAPTURE, RING_RED_TRIPLE_PULSE);
        s_busy = false;
        return;
    }

    uint32_t w = 0, h = 0;
    board_camera_last_size(&w, &h);
    ESP_LOGI(TAG, "captured %ux%u, %u bytes -> /last.jpg",
             (unsigned)w, (unsigned)h, (unsigned)len);
    s_count++;

    /*
     * Onto flash before the green flash, not after: the flash is the promise
     * that the photo is safe, and it should not be made until it is. The write
     * is a few hundred kilobytes to LittleFS - well under a second - and no
     * part of it can block on the network.
     */
    const uint8_t *body = NULL;
    size_t body_len = 0;
    esp_err_t serr = ESP_FAIL;
    if (mirror_press_last_acquire(&body, &body_len)) {
        serr = mirror_spool_add(body, body_len, trigger);
        mirror_press_last_release();
    }
    if (serr != ESP_OK) {
        ESP_LOGE(TAG, "could not spool the photo: %s - it is only at /last.jpg",
                 esp_err_to_name(serr));
        mirror_ring_play(RING_RED_TRIPLE_PULSE, mirror_pair_ring());
        s_busy = false;
        return;
    }

    /* Hand the ring back. The spool has already been told there is something
     * to send, so its green pulse takes over from underneath - there is no
     * separate "committed" flash any more, because the white shutter flash is
     * the cue and a second one just after it read as noise. */
    mirror_ring_layer_clear(RING_LAYER_CAPTURE);
    s_busy = false;
}

/*
 * Gesture detection, ported from crates/mirror-core/src/button.rs.
 *
 *   short press                         capture, or confirm a pairing attempt
 *   hold 2 s                            the ring starts filling amber, so the
 *                                       hold can be seen and cancelled
 *   hold 5 s                            enter pairing, from any state
 *   hold 20 s, release, 3 clicks in 3 s full reset
 *
 * The state lives in one struct rather than in the shape of the loop, because
 * the loop shape is what made the old "one hold is one photo" rule impossible
 * to extend.
 */
typedef struct {
    int64_t pressed_since;   /* ms, or -1 when the button is up */
    bool    long_emitted;
    bool    reset_emitted;
    bool    fill_shown;      /* the ring is showing a hold in progress */
    bool    reset_window;    /* counting clicks after a 20 s hold */
    int64_t window_opened;
    uint8_t clicks;
    bool    window_was_pressed;
} gesture_t;

static int64_t now_ms(void) { return esp_timer_get_time() / 1000; }

/* Restore whatever the ring should be showing now that a gesture is over. */
static void restore_ring(void)
{
    mirror_ring_set(mirror_pair_ring());
}

static void on_short_press(void)
{
    /* Pairing needs the press for its own confirmation, and swallows it while
     * it is running so a capture cannot start from under it. */
    if (mirror_pair_confirm()) {
        return;
    }
    if (!s_busy) {
        mirror_press_run(MIRROR_TRIGGER_BUTTON);
    }
}

static void gesture_poll(gesture_t *g, bool pressed)
{
    const int64_t now = now_ms();

    if (g->reset_window) {
        if (now - g->window_opened > RESET_CLICK_WINDOW_MS) {
            g->reset_window = false;
            ESP_LOGI(TAG, "reset aborted - no third click");
            restore_ring();
            return;
        }
        bool edge = pressed && !g->window_was_pressed;
        g->window_was_pressed = pressed;
        if (edge) {
            g->clicks++;
            if (g->clicks >= RESET_CLICK_COUNT) {
                g->reset_window = false;
                mirror_pair_factory_reset();   /* does not return */
                return;
            }
            ESP_LOGW(TAG, "reset click %u of %d", g->clicks, RESET_CLICK_COUNT);
            mirror_ring_play(RING_RED_FLASH, RING_SOLID_RED);
        }
        return;
    }

    if (g->pressed_since < 0) {
        if (pressed) {
            g->pressed_since = now;
            g->long_emitted = false;
            g->reset_emitted = false;
            g->fill_shown = false;
        }
        return;
    }

    const int64_t held = now - g->pressed_since;

    if (pressed) {
        if (held >= HOLD_RESET_MS && !g->reset_emitted) {
            g->reset_emitted = true;
            ESP_LOGW(TAG, "reset armed - release and click three times to wipe");
            mirror_ring_set(RING_SOLID_RED);
        } else if (held >= HOLD_PAIR_MS && !g->long_emitted) {
            g->long_emitted = true;
            ESP_LOGI(TAG, "long press - entering pairing");
            mirror_pair_start();
        } else if (held >= HOLD_ARM_MS && !g->long_emitted) {
            uint32_t span = HOLD_PAIR_MS - HOLD_ARM_MS;
            uint32_t percent = (uint32_t)((held - HOLD_ARM_MS) * 100 / span);
            mirror_ring_fill((uint8_t)(percent > 100 ? 100 : percent));
            g->fill_shown = true;
        }
        return;
    }

    /* Released. */
    g->pressed_since = -1;
    if (g->reset_emitted) {
        g->reset_window = true;
        g->window_opened = now;
        g->clicks = 0;
        g->window_was_pressed = false;
        return;
    }
    if (held < HOLD_PAIR_MS) {
        if (g->fill_shown) {
            /* Let go before five seconds: the hold was cancelled. */
            restore_ring();
        }
        on_short_press();
        return;
    }
    /* Between 5 s and 20 s: the long press already fired on the way past. */
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

    gesture_t g = { .pressed_since = -1 };

    /* Debounced level: the raw pin has to hold a new value for DEBOUNCE_MS
     * before the detector is told about it. */
    bool debounced = false;
    bool candidate = false;
    int64_t candidate_since = 0;

    for (;;) {
        bool raw = gpio_get_level(pin) == 0;   /* active low */
        if (raw != candidate) {
            candidate = raw;
            candidate_since = now_ms();
        } else if (raw != debounced && now_ms() - candidate_since >= DEBOUNCE_MS) {
            debounced = raw;
        }

        gesture_poll(&g, debounced);
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
