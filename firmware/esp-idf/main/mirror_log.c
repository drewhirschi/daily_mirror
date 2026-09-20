#include <stdio.h>
#include <stdarg.h>
#include <string.h>
#include <time.h>

#include "mirror_log.h"
#include "mirror_net.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "esp_system.h"
#include "esp_app_desc.h"
#include "sdkconfig.h"

static const char *TAG = "mirror_log";

#define RING_CAP (8 * 1024)

/* A whole formatted log line, on the caller's stack. Anything longer is
 * truncated into the ring; the console still gets it in full, because the
 * original vprintf is handed the untouched va_list. */
#define MIRROR_LOG_LINE_MAX 256

static char *s_ring;                 /* PSRAM, RING_CAP bytes */
static size_t s_head;                /* next write offset */
static size_t s_used;                /* bytes valid, <= RING_CAP */
static vprintf_like_t s_next;        /* the sink we replaced */
static portMUX_TYPE s_lock = portMUX_INITIALIZER_UNLOCKED;

static void ring_write(const char *s, size_t n)
{
    if (n == 0) {
        return;
    }
    if (n > RING_CAP) {
        /* Keep the tail: the end of a long line is where the interesting part
         * usually is. */
        s += n - RING_CAP;
        n = RING_CAP;
    }
    portENTER_CRITICAL(&s_lock);
    size_t first = RING_CAP - s_head;
    if (first > n) {
        first = n;
    }
    memcpy(s_ring + s_head, s, first);
    if (n > first) {
        memcpy(s_ring, s + first, n - first);
    }
    s_head = (s_head + n) % RING_CAP;
    s_used += n;
    if (s_used > RING_CAP) {
        s_used = RING_CAP;
    }
    portEXIT_CRITICAL(&s_lock);
}

static int log_hook(const char *fmt, va_list ap)
{
    /* The console first and always: whatever happens below, the serial log is
     * the one that must not regress. It needs its own copy of the list. */
    va_list console;
    va_copy(console, ap);
    int written = s_next ? s_next(fmt, console) : vprintf(fmt, console);
    va_end(console);

    /* Not from an interrupt. esp_log's ISR-safe path can reach here, and PSRAM
     * is unreachable whenever the flash cache is disabled - which is exactly
     * the situation a driver ISR logging an error is likely to be in. */
    if (s_ring && !xPortInIsrContext()) {
        char line[MIRROR_LOG_LINE_MAX];
        int n = vsnprintf(line, sizeof(line), fmt, ap);
        if (n > 0) {
            ring_write(line, (size_t)n < sizeof(line) - 1 ? (size_t)n : sizeof(line) - 1);
        }
    }
    return written;
}

esp_err_t mirror_log_init(void)
{
    if (s_ring) {
        return ESP_OK;
    }
    s_ring = heap_caps_malloc(RING_CAP, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!s_ring) {
        /* Not fatal, and deliberately not a fallback into internal RAM: 8 KB
         * of the ~156 KB this firmware has left is not a trade worth making
         * for a debug convenience. */
        ESP_LOGW(TAG, "no PSRAM for the log ring - /logs will be empty");
        return ESP_ERR_NO_MEM;
    }
    s_head = 0;
    s_used = 0;
    s_next = esp_log_set_vprintf(log_hook);
    return ESP_OK;
}

void mirror_log_usage(size_t *used, size_t *capacity)
{
    if (used) {
        *used = s_ring ? s_used : 0;
    }
    if (capacity) {
        *capacity = s_ring ? RING_CAP : 0;
    }
}

size_t mirror_log_copy(char *out, size_t cap)
{
    if (!out || cap == 0) {
        return 0;
    }
    out[0] = '\0';
    if (!s_ring) {
        return 0;
    }

    portENTER_CRITICAL(&s_lock);
    size_t used = s_used;
    size_t head = s_head;
    portEXIT_CRITICAL(&s_lock);

    if (used > cap - 1) {
        used = cap - 1;
    }
    /* Oldest byte of the newest `used` bytes. */
    size_t start = (head + RING_CAP - used) % RING_CAP;
    size_t first = RING_CAP - start;
    if (first > used) {
        first = used;
    }
    /* Reading outside the critical section: a writer that laps us can smear
     * one line. Copying 8 KB with interrupts off, on every /logs request,
     * would be worse. */
    memcpy(out, s_ring + start, first);
    if (used > first) {
        memcpy(out + first, s_ring, used - first);
    }
    out[used] = '\0';
    return used;
}

static const char *reset_reason_name(void)
{
    switch (esp_reset_reason()) {
    case ESP_RST_POWERON:  return "power-on";
    case ESP_RST_EXT:      return "external-pin";
    case ESP_RST_SW:       return "software";
    case ESP_RST_PANIC:    return "panic";
    case ESP_RST_INT_WDT:  return "interrupt-watchdog";
    case ESP_RST_TASK_WDT: return "task-watchdog";
    case ESP_RST_WDT:      return "other-watchdog";
    case ESP_RST_DEEPSLEEP: return "deep-sleep-wake";
    case ESP_RST_BROWNOUT: return "brownout";
    case ESP_RST_SDIO:     return "sdio";
    default:               return "unknown";
    }
}

void mirror_log_banner(void)
{
    const esp_app_desc_t *app = esp_app_get_description();
    time_t now = time(NULL);
    struct tm utc;
    char stamp[24];
    gmtime_r(&now, &utc);
    strftime(stamp, sizeof(stamp), "%Y-%m-%dT%H:%M:%SZ", &utc);

    ESP_LOGI(TAG, "boot: fw=%s app=%s %s %s reset=%s clock=%s (%s)",
             CONFIG_MIRROR_FW_VERSION, app ? app->version : "?",
             app ? app->date : "", app ? app->time : "",
             reset_reason_name(),
             mirror_net_clock_valid() ? "set" : "unset", stamp);
}

static esp_err_t logs_handler(httpd_req_t *req)
{
    size_t used = 0, capacity = 0;
    mirror_log_usage(&used, &capacity);

    httpd_resp_set_type(req, "text/plain");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");

    if (capacity == 0) {
        return httpd_resp_sendstr(req, "log ring unavailable (no PSRAM at start-up)\n");
    }

    /* The snapshot goes in PSRAM, not on the handler's 12 KB stack. */
    char *buf = heap_caps_malloc(capacity + 1, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!buf) {
        httpd_resp_send_err(req, HTTPD_500_INTERNAL_SERVER_ERROR, "no memory");
        return ESP_FAIL;
    }
    size_t n = mirror_log_copy(buf, capacity + 1);
    esp_err_t err = httpd_resp_send(req, buf, n);
    free(buf);
    return err;
}

esp_err_t mirror_log_register_http(httpd_handle_t server)
{
    static const httpd_uri_t uri = {
        .uri = "/logs", .method = HTTP_GET, .handler = logs_handler,
    };
    return httpd_register_uri_handler(server, &uri);
}
