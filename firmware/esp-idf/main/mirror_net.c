#include <string.h>

#include "mirror_net.h"
#include "mirror_board.h"
#include "mirror_config.h"
#include "mirror_mdns.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/event_groups.h"
#include "esp_log.h"
#include "esp_wifi.h"
#include "esp_netif.h"
#include "esp_netif_sntp.h"
#include "esp_event.h"
#include "esp_sntp.h"

static const char *TAG = "mirror_net";

#define STA_GOT_IP_BIT BIT0

static EventGroupHandle_t s_events;
static esp_netif_t *s_sta_netif;
static esp_netif_t *s_ap_netif;
static char s_sta_ip[16] = "0.0.0.0";
static char s_ap_ssid[24];
static volatile bool s_sta_connected;
static volatile bool s_ap_active;
static volatile bool s_want_sta;
static bool s_sntp_started;

/*
 * The clock. Nothing on this board keeps time across a reboot, so until SNTP
 * answers, time() is 1970 plus the uptime.
 *
 * That matters more than it sounds: a capture ID is a UTC timestamp, and the
 * server stores photos.captured_at by slicing that ID apart. A device with no
 * clock therefore files every photo under 1970 - and, worse, two devices (or
 * one device across two boots) that press the button at the same number of
 * seconds since boot produce the same ID, which the catalog's
 * ON CONFLICT(id) DO UPDATE treats as a re-upload of the same photo.
 */
static void start_sntp(void)
{
    if (s_sntp_started) {
        return;
    }
    esp_sntp_config_t cfg = ESP_NETIF_SNTP_DEFAULT_CONFIG("pool.ntp.org");
    /* Re-resolve and re-sync after a router reboot hands us a new lease. */
    /* A public pool server, not the one DHCP offers: taking the DHCP option
     * needs CONFIG_LWIP_DHCP_GET_NTP_SRV, and asking for it without that
     * fails esp_netif_sntp_init outright (ESP_ERR_INVALID_ARG). */
    cfg.start = true;
    cfg.smooth_sync = false;
    esp_err_t err = esp_netif_sntp_init(&cfg);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "sntp init failed: %s", esp_err_to_name(err));
        return;
    }
    s_sntp_started = true;
    ESP_LOGI(TAG, "sntp started - photos are timestamped once it answers");
}

bool mirror_net_clock_valid(void)
{
    time_t now = time(NULL);
    /* Any plausible wall clock is past 2024-01-01; 1970 + uptime is not. */
    return now > 1704067200;
}

static const char *reason_hint(uint8_t reason)
{
    switch (reason) {
    case WIFI_REASON_NO_AP_FOUND:
        return "SSID not seen (the P4's C6 radio is 2.4 GHz only, so a "
               "5 GHz-only SSID is invisible to it)";
    case WIFI_REASON_AUTH_FAIL:
    case WIFI_REASON_HANDSHAKE_TIMEOUT:
    case WIFI_REASON_4WAY_HANDSHAKE_TIMEOUT:
        return "authentication failed - almost always a wrong password";
    case WIFI_REASON_ASSOC_FAIL:
        return "the AP refused the association (MAC filtering? band steering?)";
    case WIFI_REASON_AUTH_EXPIRE:
    case WIFI_REASON_ASSOC_EXPIRE:
        return "dropped after associating - weak signal, or the AP aged us out";
    default:
        return "see WIFI_REASON_* in esp_wifi_types.h";
    }
}

/*
 * mirror-<last 3 MAC bytes>, the same name mirror_mdns publishes, so the
 * network someone joins to set the device up is called what the device will
 * be called once it is set up.
 *
 * It is built from mirror_device_id() rather than esp_read_mac() because on
 * the P4 the station MAC lives on the C6: esp_read_mac(ESP_MAC_WIFI_STA)
 * answers "mac type is incorrect (not found)" until the Wi-Fi driver has
 * started and fetched it over SDIO, and mirror_device_id already has the
 * fallback for that. Hence also why this is called after esp_wifi_start()
 * rather than before it.
 */
static void fill_ap_ssid(void)
{
    char id[13];
    mirror_device_id(id);
    if (strcmp(id, "000000000000") == 0) {
        /* No MAC at all. A fixed name is still better than an invisible
         * network, and it only collides if two unidentifiable devices are in
         * the same room. */
        snprintf(s_ap_ssid, sizeof(s_ap_ssid), "mirror-setup");
    } else {
        snprintf(s_ap_ssid, sizeof(s_ap_ssid), "mirror-%s", id + 6);
    }
}

static void on_wifi_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    static int retries;

    if (id == WIFI_EVENT_STA_START) {
        /* From the event, not from the caller: connecting before the driver
         * has finished starting returns ESP_ERR_WIFI_NOT_STARTED. */
        esp_wifi_connect();
        return;
    }
    if (id == WIFI_EVENT_STA_DISCONNECTED) {
        wifi_event_sta_disconnected_t *e = data;
        s_sta_connected = false;
        strcpy(s_sta_ip, "0.0.0.0");
        if (!s_want_sta) {
            return;
        }
        retries++;
        ESP_LOGW(TAG, "disconnected, reason %u: %s (attempt %d)",
                 e->reason, reason_hint(e->reason), retries);
        /* Never give up. Fast for the first few - a router reboot is over in
         * seconds - then back off so a genuinely wrong password is not a
         * busy loop for the rest of the device's life. */
        vTaskDelay(pdMS_TO_TICKS(retries < 5 ? 1000 : 10000));
        esp_wifi_connect();
        return;
    }
    if (id == WIFI_EVENT_AP_STACONNECTED) {
        wifi_event_ap_staconnected_t *e = data;
        ESP_LOGI(TAG, "a client joined the setup network (aid %d)", e->aid);
    }
}

static void on_ip_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    if (id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = data;
        snprintf(s_sta_ip, sizeof(s_sta_ip), IPSTR, IP2STR(&e->ip_info.ip));
        ESP_LOGI(TAG, "station has %s (gateway " IPSTR ")", s_sta_ip, IP2STR(&e->ip_info.gw));
        s_sta_connected = true;
        start_sntp();
        xEventGroupSetBits(s_events, STA_GOT_IP_BIT);
    }
}

/*
 * Bring up the fallback access point.
 *
 * The address matters more than it looks. Espressif's default AP subnet is
 * 192.168.4.0/24, and this household's LAN is 192.168.4.0/22 - so a phone that
 * joins the setup network while also knowing the house network ends up with
 * two routes to overlapping prefixes and reaches neither reliably. 10.10.0.1/24
 * collides with nothing here.
 */
static esp_err_t start_ap(void)
{
    if (s_ap_active) {
        return ESP_OK;
    }
    if (!s_ap_netif) {
        s_ap_netif = esp_netif_create_default_wifi_ap();
    }
    if (!s_ap_netif) {
        return ESP_FAIL;
    }

    esp_netif_ip_info_t ip = { 0 };
    ip.ip.addr = esp_ip4addr_aton("10.10.0.1");
    ip.gw.addr = esp_ip4addr_aton("10.10.0.1");
    ip.netmask.addr = esp_ip4addr_aton("255.255.255.0");
    /* The DHCP server has to be stopped before the interface can be
     * readdressed, and restarted afterwards or clients get no lease. */
    esp_netif_dhcps_stop(s_ap_netif);
    ESP_ERROR_CHECK(esp_netif_set_ip_info(s_ap_netif, &ip));
    ESP_ERROR_CHECK(esp_netif_dhcps_start(s_ap_netif));

    /* APSTA, not AP, when there are credentials to keep trying: the station
     * has to stay alive underneath so the device rejoins the house network on
     * its own once the router or the password is fixed. */
    ESP_ERROR_CHECK(esp_wifi_set_mode(s_want_sta ? WIFI_MODE_APSTA : WIFI_MODE_AP));
    if (!s_want_sta) {
        /* The station path has already started the driver. */
        ESP_ERROR_CHECK(esp_wifi_start());
    }

    /* Named only now: see fill_ap_ssid() - the MAC is not readable on the P4
     * until the driver is up. */
    fill_ap_ssid();

    wifi_config_t ap = { 0 };
    strncpy((char *)ap.ap.ssid, s_ap_ssid, sizeof(ap.ap.ssid));
    ap.ap.ssid_len = strlen(s_ap_ssid);
    strncpy((char *)ap.ap.password, CONFIG_MIRROR_AP_PASSWORD, sizeof(ap.ap.password) - 1);
    ap.ap.channel = 6;
    ap.ap.max_connection = 4;
    ap.ap.authmode = strlen(CONFIG_MIRROR_AP_PASSWORD) >= 8 ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &ap));
    s_ap_active = true;
    ESP_LOGW(TAG, "setup network \"%s\" is up on http://10.10.0.1/config", s_ap_ssid);
    return ESP_OK;
}

esp_err_t mirror_net_start(void)
{
    esp_err_t err = board_net_init();
    if (err != ESP_OK) {
        return err;
    }

    s_events = xEventGroupCreate();
    if (!s_events) {
        return ESP_ERR_NO_MEM;
    }

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    s_sta_netif = esp_netif_create_default_wifi_sta();

    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        WIFI_EVENT, ESP_EVENT_ANY_ID, &on_wifi_event, NULL, NULL));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(
        IP_EVENT, IP_EVENT_STA_GOT_IP, &on_ip_event, NULL, NULL));

    wifi_init_config_t init_cfg = WIFI_INIT_CONFIG_DEFAULT();
    err = esp_wifi_init(&init_cfg);
    if (err != ESP_OK) {
        /* On the P4 this is the C6 not answering over SDIO - a radio problem,
         * not a network one, and nothing below will help. */
        ESP_LOGE(TAG, "esp_wifi_init failed: %s", esp_err_to_name(err));
        return err;
    }

    s_want_sta = mirror_config_has_wifi();

    if (!s_want_sta) {
        ESP_LOGW(TAG, "no Wi-Fi SSID configured - starting the setup network");
        return start_ap();
    }

    const char *ssid = mirror_config_get(MIRROR_CFG_WIFI_SSID);
    wifi_config_t sta = { 0 };
    strncpy((char *)sta.sta.ssid, ssid, sizeof(sta.sta.ssid) - 1);
    strncpy((char *)sta.sta.password, mirror_config_get(MIRROR_CFG_WIFI_PASS),
            sizeof(sta.sta.password) - 1);
    /* threshold.authmode stays at its default (open): raising it to WPA2_PSK,
     * as Espressif's examples do, silently filters weaker APs out and reports
     * it as NO_AP_FOUND, which reads as a missing access point rather than a
     * policy decision. */
    sta.sta.sae_pwe_h2e = WPA3_SAE_PWE_BOTH;

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &sta));
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_LOGI(TAG, "joining \"%s\" ...", ssid);

    EventBits_t bits = xEventGroupWaitBits(s_events, STA_GOT_IP_BIT, pdFALSE, pdFALSE,
                                           pdMS_TO_TICKS(CONFIG_MIRROR_STA_TIMEOUT_S * 1000));
    if (bits & STA_GOT_IP_BIT) {
        /* Latency and steady throughput over idle power: power save has the
         * access point buffer traffic between beacons, which turns a stream of
         * frames into bursts - the opposite of what an upload wants. */
        esp_wifi_set_ps(WIFI_PS_NONE);
        return ESP_OK;
    }

    ESP_LOGW(TAG, "no address after %d s - bringing up the setup network; "
                  "the station keeps trying underneath",
             CONFIG_MIRROR_STA_TIMEOUT_S);
    return start_ap();
}

void mirror_net_restore_ap_mode(void)
{
    if (!s_ap_active) {
        return;
    }
    esp_err_t err = esp_wifi_set_mode(WIFI_MODE_APSTA);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "could not restore the setup network: %s", esp_err_to_name(err));
    }
}

esp_err_t mirror_net_adopt_sta_credentials(void)
{
    wifi_config_t sta = { 0 };
    esp_err_t err = esp_wifi_get_config(WIFI_IF_STA, &sta);
    if (err != ESP_OK) {
        return err;
    }
    if (sta.sta.ssid[0] == '\0') {
        return ESP_ERR_INVALID_STATE;
    }

    /* NUL-terminate: the driver's fields are fixed-width and need not be. */
    char ssid[33] = { 0 };
    char pass[65] = { 0 };
    memcpy(ssid, sta.sta.ssid, sizeof(sta.sta.ssid));
    memcpy(pass, sta.sta.password, sizeof(sta.sta.password));

    err = mirror_config_set(MIRROR_CFG_WIFI_SSID, ssid);
    if (err == ESP_OK) {
        err = mirror_config_set(MIRROR_CFG_WIFI_PASS, pass);
    }
    if (err != ESP_OK) {
        return err;
    }

    /* From here on a disconnect is worth retrying: these are credentials the
     * device is meant to keep, not something the provisioning manager is
     * still trying out. */
    s_want_sta = true;
    ESP_LOGI(TAG, "adopted the paired network \"%s\"", ssid);
    return ESP_OK;
}

bool mirror_net_sta_connected(void) { return s_sta_connected; }
bool mirror_net_ap_active(void)     { return s_ap_active; }
const char *mirror_net_ip(void)     { return s_sta_connected ? s_sta_ip : "10.10.0.1"; }
const char *mirror_net_ap_ssid(void) { return s_ap_active ? s_ap_ssid : ""; }
