#include "mirror_ring.h"
#include "mirror_board.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

/* The renderer's step, and the unit every duration below is counted in. */
#define TICK_MS 25

/*
 * Countdown: four pulses at one a second.
 *
 * The Pi's was three at 650/350, and it was followed on this board by three
 * more seconds of rapid amber while autofocus ran - so the countdown did not
 * actually count down to anything. The camera work now happens underneath
 * these blinks (mirror_press.c), and the shutter fires as the last one ends,
 * which is what makes "get ready" mean something.
 */
#define COUNTDOWN_ON_MS   350
#define COUNTDOWN_OFF_MS  650
#define COUNTDOWN_PULSES  4
#define GREEN_FLASH_MS    700
#define GREEN_PULSE_MS    120     /* one flash of the green triple */
#define GREEN_TRIPLES     3
#define RED_PULSE_MS      150
#define RED_PULSES        3
#define RED_FLASH_MS      120

/* One sweep of the pairing chase, and the fast pulse the confirm press waits
 * behind. The chase is deliberately slower than the confirm pulse: "come and
 * find me" versus "do something now". */
#define CHASE_PERIOD_MS   1200
#define FAST_PULSE_MS     250

static volatile ring_pattern_t s_pattern = RING_OFF;   /* the BASE layer */
static volatile uint32_t s_epoch;   /* bumped on every set, to restart animations */
static volatile uint8_t s_fill_percent;

/* The layers above BASE. RING_OFF here means "nothing to say", not "go dark" -
 * a layer that wants darkness simply does not claim the ring. */
static volatile ring_pattern_t s_upload  = RING_OFF;
static volatile ring_pattern_t s_capture = RING_OFF;

/*
 * Patterns that BASE is allowed to hold over the upload layer.
 *
 * All of them are pairing or failure: "the app is looking for me", "press the
 * button to confirm", a hold in progress, an armed reset, a hard fault. A
 * photo going out in the background must not paint over any of those.
 */
static bool base_outranks_upload(ring_pattern_t base)
{
    switch (base) {
    case RING_AMBER_CHASE:
    case RING_AMBER_FAST_PULSE:
    case RING_AMBER_FILL:
    case RING_RED_FLASH:
    case RING_SOLID_RED:
    case RING_DIM_WHITE_BREATHE:
        return true;
    default:
        return false;
    }
}

static ring_pattern_t effective(void)
{
    if (s_capture != RING_OFF) {
        return s_capture;
    }
    if (s_upload != RING_OFF && !base_outranks_upload(s_pattern)) {
        return s_upload;
    }
    return s_pattern;
}

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
    case RING_GREEN_SLOW_PULSE: {
        /* The same breathing shape as BLUE_SLOW_PULSE, in green. Blue stays
         * what it has always meant - joining a network, claiming - and green
         * now carries "this photo is on its way", so a glance at the ring says
         * which of the two the device is busy with. A deliberate divergence
         * from crates/mirror-core/src/ring.rs, where uploading is blue; noted
         * in firmware/esp-idf/README.md. */
        uint32_t phase = ms % 2000;
        uint32_t up = phase < 1000 ? phase : 2000 - phase;
        board_led_set_rgb(0, (uint8_t)(6 + (up * 60) / 1000), 0);
        break;
    }
    case RING_GREEN_TRIPLE_FLASH:
        if ((ms / GREEN_PULSE_MS) % 2 == 0) {
            board_led_set_rgb(0, 100, 0);
        } else {
            board_led_set_rgb(0, 0, 0);
        }
        break;
    case RING_WHITE_FLASH:
        /* The shutter. One bright, short, unmistakable cue at the instant the
         * frame is taken - the thing a countdown has to end in. */
        board_led_set_rgb(120, 120, 120);
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
    case RING_AMBER_CHASE: {
        /* A chase on a one-LED "ring": amber ramps up over the sweep and drops
         * back, which reads as movement rather than as a pulse. */
        uint32_t up = ms % CHASE_PERIOD_MS;             /* 0..1199 */
        uint8_t v = (uint8_t)(6 + (up * 74) / CHASE_PERIOD_MS);
        board_led_set_rgb(v, (uint8_t)(v / 2), 0);
        break;
    }
    case RING_AMBER_FAST_PULSE:
        if ((ms / FAST_PULSE_MS) % 2 == 0) {
            board_led_set_rgb(90, 45, 0);
        } else {
            board_led_set_rgb(6, 3, 0);
        }
        break;
    case RING_AMBER_FILL: {
        /* Not animated: the level is the fraction of the hold completed, so
         * releasing early is visibly a cancel. */
        uint8_t v = (uint8_t)(6 + ((uint32_t)s_fill_percent * 84) / 100);
        board_led_set_rgb(v, (uint8_t)(v / 2), 0);
        break;
    }
    case RING_RED_FLASH:
        board_led_set_rgb(110, 0, 0);
        break;
    }
}

static void ring_task(void *arg)
{
    (void)arg;
    uint32_t epoch = s_epoch;
    ring_pattern_t shown = effective();
    uint32_t t = 0;
    for (;;) {
        ring_pattern_t now = effective();
        /* Restart the animation when the winning layer changes, so a pattern
         * that comes back from underneath starts at its beginning rather than
         * halfway through a pulse. */
        if (now != shown || epoch != s_epoch) {
            shown = now;
            epoch = s_epoch;
            t = 0;
        }
        render(shown, t++);
        vTaskDelay(pdMS_TO_TICKS(TICK_MS));
    }
}

/** The natural length of a pattern that has one. */
static uint32_t pattern_ms(ring_pattern_t pattern)
{
    switch (pattern) {
    case RING_AMBER_SLOW_PULSE:
        return COUNTDOWN_PULSES * (COUNTDOWN_ON_MS + COUNTDOWN_OFF_MS);
    case RING_GREEN_FLASH:
        return GREEN_FLASH_MS;
    case RING_GREEN_TRIPLE_FLASH:
        return GREEN_TRIPLES * 2 * GREEN_PULSE_MS;
    case RING_WHITE_FLASH:
        return MIRROR_RING_SNAP_MS;
    case RING_RED_TRIPLE_PULSE:
        return RED_PULSES * 2 * RED_PULSE_MS;
    case RING_RED_FLASH:
        return RED_FLASH_MS;
    default:
        return 500;
    }
}

void mirror_ring_layer_set(ring_layer_t layer, ring_pattern_t pattern)
{
    if (layer == RING_LAYER_CAPTURE) {
        s_capture = pattern;
    } else if (layer == RING_LAYER_UPLOAD) {
        s_upload = pattern;
    } else {
        mirror_ring_set(pattern);
        return;
    }
    s_epoch++;
}

void mirror_ring_layer_clear(ring_layer_t layer)
{
    mirror_ring_layer_set(layer, RING_OFF);
}

void mirror_ring_layer_play(ring_layer_t layer, ring_pattern_t pattern)
{
    mirror_ring_layer_set(layer, pattern);
    vTaskDelay(pdMS_TO_TICKS(pattern_ms(pattern)));
    /* Only clear it if nobody has claimed the layer since - a press that
     * arrived mid-flash owns it now. */
    if ((layer == RING_LAYER_CAPTURE ? s_capture : s_upload) == pattern) {
        mirror_ring_layer_clear(layer);
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
    mirror_ring_set(pattern);
    vTaskDelay(pdMS_TO_TICKS(pattern_ms(pattern)));
    mirror_ring_set(next);
}

void mirror_ring_fill(uint8_t percent)
{
    s_fill_percent = percent > 100 ? 100 : percent;
    /* Deliberately not mirror_ring_set(): this arrives on every 20 ms button
     * poll, and bumping the epoch each time would hold every animation at
     * t = 0 for as long as the button is held. */
    s_pattern = RING_AMBER_FILL;
}
