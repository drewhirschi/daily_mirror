#include <inttypes.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "mirror_link.h"
#include "sdkconfig.h"

#if CONFIG_ESP_HOSTED_ENABLED

#include "mirror_config.h"
#include "mirror_net.h"
#include "mirror_pair.h"
#include "mirror_spool.h"
#include "mirror_upload.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_attr.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_wifi.h"
#include "esp_hosted.h"
#include "esp_hosted_event.h"

static const char *TAG = "mirror_link";

#define TICK_MS              5000
#define HEARTBEAT_S          10
/* Three missed beats and change. One late beat is a busy C6, not a dead one. */
#define HEARTBEAT_TIMEOUT_S  45
#define MIRROR_LINK_STALL_S  180
#define STALL_MAX_S          3600

/*
 * Kept across esp_restart() (not across a power cut), so the boot after a
 * watchdog restart can say why it happened, and so the stall threshold can
 * keep doubling across restarts instead of starting from three minutes every
 * time. A cold boot finds garbage here; the magic tells it apart.
 */
#define STATE_MAGIC 0x4c4e4b31u   /* "LNK1" */
RTC_NOINIT_ATTR static uint32_t s_rtc_magic;
RTC_NOINIT_ATTR static uint32_t s_rtc_stall_level;
RTC_NOINIT_ATTR static char     s_rtc_reason[96];

static volatile int64_t s_last_beat_us;   /* 0 = no heartbeat seen yet */
static volatile uint32_t s_beats;
static bool s_heartbeat_on;

/* Log it, keep it for the next boot, and go. The short delay lets the line
 * reach the UART and the log ring before the restart cuts them off. */
static void restart_for(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void restart_for(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(s_rtc_reason, sizeof(s_rtc_reason), fmt, ap);
    va_end(ap);
    s_rtc_magic = STATE_MAGIC;
    ESP_LOGE(TAG, "restarting to recover the co-processor link: %s "
                  "(%" PRIu32 " photos queued; they are on flash and survive this)",
             s_rtc_reason, mirror_spool_count());
    vTaskDelay(pdMS_TO_TICKS(200));
    esp_restart();
}

static void on_hosted_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg; (void)base;
    switch (id) {
    case ESP_HOSTED_EVENT_CP_HEARTBEAT:
        s_last_beat_us = esp_timer_get_time();
        s_beats++;
        break;
    case ESP_HOSTED_EVENT_CP_INIT:
        /* The C6 rebooted under a running host. Worth seeing in a field log;
         * if it took the link with it, the checks below will notice. */
        ESP_LOGW(TAG, "co-processor restarted (reset reason %d)",
                 data ? (int)((esp_hosted_event_init_t *)data)->reason : -1);
        break;
    case ESP_HOSTED_EVENT_TRANSPORT_FAILURE:
        /* CONFIG_ESP_HOSTED_TRANSPORT_RESTART_ON_FAILURE usually restarts the
         * host straight after posting this, and this handler may never run.
         * If it does run, the restart is ours. */
        restart_for("esp_hosted reported an SDIO transport failure");
        break;
    default:
        break;
    }
}

/*
 * The heartbeat is a slave feature (ConfigHeartbeat), and only a slave that
 * can report its version is trusted with it. The factory C6 firmware
 * predates version reporting and shows as 0.0.0. Turning on a heartbeat it
 * might not send would make a healthy link look dead.
 */
static void maybe_enable_heartbeat(void)
{
    esp_hosted_coprocessor_fwver_t v = { 0 };
    esp_err_t err = esp_hosted_get_coprocessor_fwversion(&v);
    if (err != ESP_OK || (v.major1 == 0 && v.minor1 == 0 && v.patch1 == 0)) {
        ESP_LOGW(TAG, "co-processor reports no version (factory firmware?) - heartbeat "
                      "watchdog off; the stall watchdog still runs");
        return;
    }
    err = esp_hosted_configure_heartbeat(true, HEARTBEAT_S);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "co-processor reports esp_hosted %" PRIu32 ".%" PRIu32 ".%" PRIu32
                      " but would not start a heartbeat: %s",
                 v.major1, v.minor1, v.patch1, esp_err_to_name(err));
        return;
    }
    s_heartbeat_on = true;
    ESP_LOGI(TAG, "co-processor reports esp_hosted %" PRIu32 ".%" PRIu32 ".%" PRIu32
                  " - heartbeat on, every %d s",
             v.major1, v.minor1, v.patch1, HEARTBEAT_S);
}

static uint32_t stall_limit_s(void)
{
    uint32_t s = MIRROR_LINK_STALL_S;
    for (uint32_t i = 0; i < s_rtc_stall_level && s < STALL_MAX_S; i++) {
        s *= 2;
    }
    return s < STALL_MAX_S ? s : STALL_MAX_S;
}

/* Someone on the setup network is probably mid-way through /config. A
 * restart would take the network out from under them. */
static bool ap_has_clients(void)
{
    if (!mirror_net_ap_active()) {
        return false;
    }
    wifi_sta_list_t list = { 0 };
    return esp_wifi_ap_get_sta_list(&list) == ESP_OK && list.num > 0;
}

static void link_task(void *arg)
{
    (void)arg;
    int64_t down_since_us = 0;

    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(TICK_MS));
        int64_t now = esp_timer_get_time();

        /* 1. Heartbeat. Armed by the first beat, not by the configure call:
         *    a slave that accepts the call and never beats would otherwise be
         *    restarted forever. */
        if (s_heartbeat_on && s_last_beat_us
            && now - s_last_beat_us > (int64_t)HEARTBEAT_TIMEOUT_S * 1000000) {
            restart_for("no co-processor heartbeat for %" PRId64 " s (last was #%" PRIu32 ")",
                        (now - s_last_beat_us) / 1000000, s_beats);
        }

        /* 2. Stalled with photos waiting. Nothing queued means nothing is
         *    being lost, and the backoff can start over. */
        bool waiting = mirror_spool_count() > 0 && mirror_upload_configured();
        if (!waiting) {
            s_rtc_stall_level = 0;
            down_since_us = 0;
            continue;
        }

        if (mirror_config_has_wifi() && !mirror_net_sta_connected()) {
            if (!down_since_us) {
                down_since_us = now;
            }
        } else {
            down_since_us = 0;
        }
        int64_t unreachable_since_us = mirror_spool_unreachable_since_us();

        int64_t limit_us = (int64_t)stall_limit_s() * 1000000;
        const char *why = NULL;
        int64_t since_us = 0;
        if (down_since_us && now - down_since_us > limit_us) {
            why = "station off the network";
            since_us = down_since_us;
        } else if (unreachable_since_us && now - unreachable_since_us > limit_us) {
            why = "no upload has reached the server";
            since_us = unreachable_since_us;
        }
        if (!why) {
            continue;
        }

        /* Pairing drives the radio itself and owns the button and ring.
         * Leave it alone; the check runs again once it is done. */
        if (mirror_pair_state() != MIRROR_PAIR_IDLE || ap_has_clients()) {
            continue;
        }

        s_rtc_stall_level++;
        restart_for("%s for %" PRId64 " s with %" PRIu32 " photos waiting "
                    "(next threshold %" PRIu32 " s)",
                    why, (now - since_us) / 1000000, mirror_spool_count(), stall_limit_s());
    }
}

esp_err_t mirror_link_start(void)
{
    if (s_rtc_magic != STATE_MAGIC) {
        /* Cold boot: whatever is in RTC memory is noise. */
        s_rtc_magic = STATE_MAGIC;
        s_rtc_stall_level = 0;
        s_rtc_reason[0] = '\0';
    } else if (s_rtc_reason[0]) {
        s_rtc_reason[sizeof(s_rtc_reason) - 1] = '\0';
        ESP_LOGW(TAG, "the last restart was this watchdog: %s", s_rtc_reason);
        s_rtc_reason[0] = '\0';
    }

    esp_err_t err = esp_event_handler_instance_register(
        ESP_HOSTED_EVENT, ESP_EVENT_ANY_ID, &on_hosted_event, NULL, NULL);
    if (err != ESP_OK) {
        return err;
    }

    maybe_enable_heartbeat();

    /* Small: it only polls flags and, rarely, asks the C6 for its AP client
     * list. 3 KB covers that RPC with room left over. */
    if (xTaskCreate(link_task, "link_wd", 3072, NULL, 3, NULL) != pdPASS) {
        return ESP_ERR_NO_MEM;
    }
    if (s_heartbeat_on) {
        ESP_LOGI(TAG, "link watchdog on: restart after %" PRIu32 " s stalled with photos "
                      "waiting, or %d s without a heartbeat",
                 stall_limit_s(), HEARTBEAT_TIMEOUT_S);
    } else {
        ESP_LOGI(TAG, "link watchdog on: restart after %" PRIu32 " s stalled with photos waiting",
                 stall_limit_s());
    }
    return ESP_OK;
}

#else  /* !CONFIG_ESP_HOSTED_ENABLED */

/* The S3 has its radio on-chip and no link to watch. */
esp_err_t mirror_link_start(void) { return ESP_OK; }

#endif
