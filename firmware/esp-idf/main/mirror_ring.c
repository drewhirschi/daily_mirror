#include "mirror_ring.h"
#include "mirror_board.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

/* The renderer's step, and the unit every duration below is counted in. */
#define TICK_MS 25

/* Countdown: three pulses, 650 ms on / 350 ms off, from the Pi firmware. */
#define COUNTDOWN_ON_MS   650
#define COUNTDOWN_OFF_MS  350
#define COUNTDOWN_PULSES  3
#define GREEN_FLASH_MS    700
#define RED_PULSE_MS      150
#define RED_PULSES        3

static volatile ring_pattern_t s_pattern = RING_OFF;
static volatile uint32_t s_epoch;   /* bumped on every set, to restart animations */

static void render(ring_pattern_t p, uint32_t t /* ticks since the pattern began */)
{
    const uint32_t ms = t * TICK_MS;

    switch (p) {
    case RING_OFF:
        board_led_set_rgb(0, 0, 0);
        break;
    case RING_SOLID_WHITE:
        board_led_set_rgb(12, 12, 12);
        break;
    case RING_DIM_WHITE_BREATHE: {
        /* ~4 s period, 2..14. Integer triangle wave, no float and no libm. */
        uint32_t phase = ms % 4000;
        uint32_t up = phase < 2000 ? phase : 4000 - phase;   /* 0..2000 */
        uint8_t v = (uint8_t)(2 + (up * 12) / 2000);
        board_led_set_rgb(v, v, v);
        break;
    }
    case RING_AMBER_SLOW_PULSE: {
        uint32_t period = COUNTDOWN_ON_MS + COUNTDOWN_OFF_MS;
        if (ms % period < COUNTDOWN_ON_MS) {
            board_led_set_rgb(80, 40, 0);
        } else {
            board_led_set_rgb(0, 0, 0);
        }
        break;
    }
    case RING_AMBER_RAPID_PULSE:
        if ((ms / 120) % 2) {
            board_led_set_rgb(80, 40, 0);
        } else {
            board_led_set_rgb(0, 0, 0);
        }
        break;
    case RING_GREEN_FLASH:
        board_led_set_rgb(0, 90, 0);
        break;
    case RING_BLUE_SLOW_PULSE: {
        uint32_t phase = ms % 2000;
        uint32_t up = phase < 1000 ? phase : 2000 - phase;
        board_led_set_rgb(0, 0, (uint8_t)(10 + (up * 50) / 1000));
        break;
    }
    case RING_RED_TRIPLE_PULSE:
        if ((ms / RED_PULSE_MS) % 2 == 0) {
            board_led_set_rgb(90, 0, 0);
        } else {
            board_led_set_rgb(0, 0, 0);
        }
        break;
    case RING_SOLID_RED:
        board_led_set_rgb(64, 0, 0);
        break;
    }
}

static void ring_task(void *arg)
{
    (void)arg;
    uint32_t epoch = s_epoch;
    uint32_t t = 0;
    for (;;) {
        if (epoch != s_epoch) {
            epoch = s_epoch;
            t = 0;
        }
        render(s_pattern, t++);
        vTaskDelay(pdMS_TO_TICKS(TICK_MS));
    }
}

esp_err_t mirror_ring_start(void)
{
    return xTaskCreate(ring_task, "ring", 2560, NULL, 4, NULL) == pdPASS
        ? ESP_OK : ESP_ERR_NO_MEM;
}

void mirror_ring_set(ring_pattern_t pattern)
{
    s_pattern = pattern;
    s_epoch++;
}

void mirror_ring_play(ring_pattern_t pattern, ring_pattern_t next)
{
    uint32_t ms;
    switch (pattern) {
    case RING_AMBER_SLOW_PULSE:
        ms = COUNTDOWN_PULSES * (COUNTDOWN_ON_MS + COUNTDOWN_OFF_MS);
        break;
    case RING_GREEN_FLASH:
        ms = GREEN_FLASH_MS;
        break;
    case RING_RED_TRIPLE_PULSE:
        ms = RED_PULSES * 2 * RED_PULSE_MS;
        break;
    default:
        ms = 500;
        break;
    }
    mirror_ring_set(pattern);
    vTaskDelay(pdMS_TO_TICKS(ms));
    mirror_ring_set(next);
}
