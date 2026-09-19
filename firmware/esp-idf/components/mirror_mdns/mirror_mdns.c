/*
 * Daily Mirror LAN discovery. See include/mirror_mdns.h for the contract.
 */

#include "mirror_mdns.h"

#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_mac.h"
#include "esp_wifi.h"
#include "mdns.h"

#define MIRROR_MDNS_SERVICE  "_dailymirror"
#define MIRROR_MDNS_PROTO    "_tcp"
#define MIRROR_MDNS_INSTANCE "Daily Mirror camera"

static const char *TAG = "mirror_mdns";

static char s_hostname[16];   /* "mirror-xxxxxx" */
static char s_device_id[13];
static bool s_started;

/*
 * Station MAC, with fallbacks.
 *
 * On the ESP32-S3 esp_read_mac(ESP_MAC_WIFI_STA) reads the eFuse base MAC and
 * derives the STA address from it. On the ESP32-P4 the radio lives on a
 * companion chip reached through esp_wifi_remote/esp_hosted, so the P4's own
 * eFuse may carry no Wi-Fi MAC at all and that call can fail. Fall back to
 * asking the (remote) Wi-Fi driver for the interface address it actually uses,
 * and finally to the base/efuse MAC so the id is at least stable per board.
 */
static uint8_t s_mac[6];
static bool    s_mac_final; /* true once a real station MAC has been resolved */

static esp_err_t read_sta_mac(uint8_t mac[6])
{
    /* The first two sources are authoritative; once one answers, keep it so the
     * device id cannot change underneath the caller. The ESP_MAC_BASE fallback
     * is deliberately NOT cached: on the P4 it is the host chip's own efuse
     * MAC, and we would rather upgrade to the radio's address once Wi-Fi is up
     * than freeze a different id for the life of the boot. */
    if (s_mac_final) {
        memcpy(mac, s_mac, 6);
        return ESP_OK;
    }

    esp_err_t err = esp_read_mac(mac, ESP_MAC_WIFI_STA);
    if (err == ESP_OK) goto found;
    ESP_LOGW(TAG, "ESP_MAC_WIFI_STA unavailable (%s); trying the Wi-Fi driver",
             esp_err_to_name(err));

    /* Only meaningful once esp_wifi_init() has run; harmless otherwise. */
    err = esp_wifi_get_mac(WIFI_IF_STA, mac);
    if (err == ESP_OK) goto found;
    ESP_LOGW(TAG, "esp_wifi_get_mac failed (%s); falling back to the base MAC",
             esp_err_to_name(err));

    err = esp_read_mac(mac, ESP_MAC_BASE);
    if (err == ESP_OK) return ESP_OK;

    ESP_LOGE(TAG, "no MAC available (%s); device id will be zeros",
             esp_err_to_name(err));
    memset(mac, 0, 6);
    return err;

found:
    memcpy(s_mac, mac, 6);
    s_mac_final = true;
    return ESP_OK;
}

void mirror_device_id(char out[13])
{
    if (out == NULL) return;
    uint8_t mac[6];
    (void)read_sta_mac(mac);
    snprintf(out, 13, "%02x%02x%02x%02x%02x%02x",
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
}

const char *mirror_mdns_hostname(void)
{
    return s_hostname;
}

esp_err_t mirror_mdns_start(const mirror_mdns_info_t *info)
{
    if (info == NULL) return ESP_ERR_INVALID_ARG;

    uint8_t mac[6];
    (void)read_sta_mac(mac);
    snprintf(s_hostname, sizeof(s_hostname), "mirror-%02x%02x%02x",
             mac[3], mac[4], mac[5]);
    snprintf(s_device_id, sizeof(s_device_id), "%02x%02x%02x%02x%02x%02x",
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

    if (!s_started) {
        esp_err_t err = mdns_init();
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "mdns_init failed: %s", esp_err_to_name(err));
            return err;
        }
        s_started = true;
    }

    ESP_ERROR_CHECK_WITHOUT_ABORT(mdns_hostname_set(s_hostname));
    ESP_ERROR_CHECK_WITHOUT_ABORT(mdns_instance_name_set(MIRROR_MDNS_INSTANCE));

    uint16_t port = info->http_port != 0 ? info->http_port : 80;
    mdns_txt_item_t txt[] = {
        { "id",      s_device_id },
        { "board",   info->board      != NULL ? info->board      : "" },
        { "fw",      info->fw_version != NULL ? info->fw_version : "" },
        { "claimed", info->claimed ? "1" : "0" },
    };
    const size_t txt_count = sizeof(txt) / sizeof(txt[0]);

    /* A second call is an update, not an error: remove and re-add so the port
     * and every TXT record match the caller's current info. */
    if (mdns_service_exists(MIRROR_MDNS_SERVICE, MIRROR_MDNS_PROTO, NULL)) {
        ESP_ERROR_CHECK_WITHOUT_ABORT(
            mdns_service_remove(MIRROR_MDNS_SERVICE, MIRROR_MDNS_PROTO));
    }
    esp_err_t err = mdns_service_add(MIRROR_MDNS_INSTANCE, MIRROR_MDNS_SERVICE,
                                     MIRROR_MDNS_PROTO, port, txt, txt_count);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "advertising %s failed: %s", MIRROR_MDNS_SERVICE,
                 esp_err_to_name(err));
        return err;
    }

    if (mdns_service_exists("_http", MIRROR_MDNS_PROTO, NULL)) {
        ESP_ERROR_CHECK_WITHOUT_ABORT(mdns_service_remove("_http", MIRROR_MDNS_PROTO));
    }
    ESP_ERROR_CHECK_WITHOUT_ABORT(
        mdns_service_add(NULL, "_http", MIRROR_MDNS_PROTO, port, NULL, 0));

    ESP_LOGI(TAG, "http://%s.local:%u/  %s.%s id=%s claimed=%d",
             s_hostname, (unsigned)port, MIRROR_MDNS_SERVICE, MIRROR_MDNS_PROTO,
             s_device_id, info->claimed ? 1 : 0);
    return ESP_OK;
}

esp_err_t mirror_mdns_set_claimed(bool claimed)
{
    if (!s_started) return ESP_ERR_INVALID_STATE;
    esp_err_t err = mdns_service_txt_item_set(MIRROR_MDNS_SERVICE, MIRROR_MDNS_PROTO,
                                              "claimed", claimed ? "1" : "0");
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "claimed=%d not advertised: %s", claimed ? 1 : 0,
                 esp_err_to_name(err));
    }
    return err;
}
