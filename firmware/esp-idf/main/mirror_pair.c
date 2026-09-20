/*
 * Device pairing. See mirror_pair.h for the protocol and the state machine it
 * ports from crates/mirror-core/src/state.rs.
 */

#include <stdlib.h>
#include <string.h>

#include "mirror_pair.h"
#include "mirror_config.h"
#include "mirror_mdns.h"
#include "mirror_net.h"
#include "mirror_ring.h"
#include "mirror_board.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_event.h"
#include "esp_wifi.h"
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "cJSON.h"
/*
 * BLE is the pairing transport, and only the ESP32-S3 has a radio for it. The
 * ESP32-P4's Wi-Fi lives on a companion ESP32-C6 and upstream does not support
 * provisioning through it yet (see docs/firmware-roadmap.md) - the
 * wifi_provisioning component is not even linked into that build - so on that
 * board this module compiles to "pairing is not available here" and the device
 * is configured through /config as before.
 */
#if CONFIG_BT_ENABLED
#include "esp_srp.h"
#include "protocomm_ble.h"
#include "wifi_provisioning/manager.h"
#include "wifi_provisioning/scheme_ble.h"
#endif

static const char *TAG = "mirror_pair";

/* Timings, from crates/mirror-core/src/lib.rs `timing`. They are the physical
 * interaction, so they are the same on every platform. */
#define PAIRING_TIMEOUT_MS  (5 * 60 * 1000)
#define CONFIRM_TIMEOUT_MS  (30 * 1000)

/* How long to keep waiting for an address once the user has confirmed. The
 * manager is joining underneath; a DHCP lease on a busy household network can
 * take a while, and the alternative is telling the user it failed when it did
 * not. */
#define JOIN_WAIT_MS        45000

/* How long the BLE service stays up after a successful claim, so the phone can
 * read the "claimed" result before the link it is reading it over disappears.
 * The app polls every 1.5 s. */
#define CLAIMED_LINGER_MS   8000

/* Loop period of the pairing task. Fine enough for a 30 s window. */
#define TICK_MS             200

/* The custom endpoint, matching PROVISIONING_ENDPOINT in
 * mobile/src/pairing/contract.ts. */
#define PAIR_ENDPOINT       "daily-mirror"

#define SRP_SALT_LEN        16

/* A claim token is ~43 characters today; a server URL is a bare origin. Both
 * are stored in settings-sized fields, so use the same bound. */
#define PAIR_FIELD_MAX      MIRROR_CONFIG_VALUE_MAX

static SemaphoreHandle_t s_lock;
static volatile mirror_pair_state_t s_state = MIRROR_PAIR_IDLE;
static volatile bool s_confirmed;
static volatile bool s_service_running;
/* Set once the BLE controller's memory has been handed back. From then on the
 * only way into pairing is a restart. */
static volatile bool s_bt_released;
/* Set between asking the service to stop and the manager finishing. */
static volatile bool s_stopping;
static TaskHandle_t s_task;

static int64_t s_state_since_us;

/* The claim payload the app sent, and the reply it polls for. */
static char s_server_url[PAIR_FIELD_MAX];
static char s_claim_token[PAIR_FIELD_MAX];
static char s_result[256] = "{\"status\":\"awaiting_confirm\"}";

#if CONFIG_BT_ENABLED
/* Generated once and handed to the manager, which keeps the pointers. */
static char *s_salt;
static char *s_verifier;
static int   s_verifier_len;
static char  s_service_name[MAX_BLE_DEVNAME_LEN + 1];
#endif

static void lock(void)   { if (s_lock) xSemaphoreTake(s_lock, portMAX_DELAY); }
static void unlock(void) { if (s_lock) xSemaphoreGive(s_lock); }

static int64_t now_ms(void) { return esp_timer_get_time() / 1000; }

static int64_t in_state_ms(void) { return now_ms() - (s_state_since_us / 1000); }

static void enter(mirror_pair_state_t state)
{
    s_state = state;
    s_state_since_us = esp_timer_get_time();
}

/* ------------------------------------------------------------- reporting --- */

/*
 * The reply the app reads back from the custom endpoint. Built with cJSON so
 * a device name with a quote in it cannot produce a body the app rejects as
 * "a reply this app did not understand".
 */
static void set_result(const char *status, const char *key, const char *value)
{
    cJSON *doc = cJSON_CreateObject();
    if (!doc) {
        return;
    }
    cJSON_AddStringToObject(doc, "status", status);
    if (key && value) {
        cJSON_AddStringToObject(doc, key, value);
    }
    char *text = cJSON_PrintUnformatted(doc);
    cJSON_Delete(doc);
    if (!text) {
        return;
    }

    lock();
    strlcpy(s_result, text, sizeof(s_result));
    unlock();
    free(text);
}

static void report_awaiting_confirm(void) { set_result("awaiting_confirm", NULL, NULL); }
static void report_claimed(const char *device_name)
{
    set_result("claimed", "device_name", device_name);
}
static void report_failed(const char *reason)
{
    ESP_LOGW(TAG, "pairing failed: %s", reason);
    set_result("failed", "reason", reason);
}

/* ------------------------------------------------------------- the claim --- */

typedef struct {
    char  *buf;
    size_t len;
} resp_buf_t;

static esp_err_t collect(esp_http_client_event_t *evt)
{
    resp_buf_t *r = evt->user_data;
    if (evt->event_id == HTTP_EVENT_ON_DATA && r) {
        /* A DeviceClaimed is a few hundred bytes. Cap it so a misconfigured
         * server_url cannot turn this into an unbounded realloc loop. */
        if (r->len + evt->data_len > 2048) {
            return ESP_FAIL;
        }
        char *n = realloc(r->buf, r->len + evt->data_len + 1);
        if (!n) {
            return ESP_FAIL;
        }
        r->buf = n;
        memcpy(r->buf + r->len, evt->data, evt->data_len);
        r->len += evt->data_len;
        r->buf[r->len] = '\0';
    }
    return ESP_OK;
}

/*
 * POST {server_url}/api/devices/claim.
 *
 * Deliberately unauthenticated: the claim token is the credential (see
 * server/app/api/devices/claim/route.rs). The body is DeviceClaimRequest and
 * the reply is DeviceClaimed, both from crates/mirror-core/src/contract.rs.
 *
 * `reason` is filled with something the app's failureMessage() can classify.
 */
static esp_err_t redeem_claim(char *reason, size_t reason_len)
{
    char device_id[13];
    mirror_device_id(device_id);

    cJSON *doc = cJSON_CreateObject();
    if (!doc) {
        snprintf(reason, reason_len, "claim: out of memory");
        return ESP_ERR_NO_MEM;
    }
    cJSON_AddStringToObject(doc, "device_id", device_id);
    cJSON_AddStringToObject(doc, "claim_token", s_claim_token);
    cJSON_AddStringToObject(doc, "firmware_version", CONFIG_MIRROR_FW_VERSION);
    cJSON_AddStringToObject(doc, "hardware", board_id());
    char *body = cJSON_PrintUnformatted(doc);
    cJSON_Delete(doc);
    if (!body) {
        snprintf(reason, reason_len, "claim: out of memory");
        return ESP_ERR_NO_MEM;
    }

    char url[PAIR_FIELD_MAX + 32];
    snprintf(url, sizeof(url), "%s/api/devices/claim", s_server_url);

    resp_buf_t resp = { 0 };
    esp_http_client_config_t cfg = {
        .url = url,
        .method = HTTP_METHOD_POST,
        .event_handler = collect,
        .user_data = &resp,
        .timeout_ms = 20000,
        /* The Mozilla root bundle compiled into the image, as the upload path
         * uses. A claim carries a token that becomes this device's identity;
         * it is not something to hand to whoever answers. */
        .crt_bundle_attach = esp_crt_bundle_attach,
    };
    esp_http_client_handle_t c = esp_http_client_init(&cfg);
    if (!c) {
        free(body);
        snprintf(reason, reason_len, "claim: http client init failed");
        return ESP_FAIL;
    }
    esp_http_client_set_header(c, "Content-Type", "application/json");
    esp_http_client_set_post_field(c, body, strlen(body));
    esp_err_t err = esp_http_client_perform(c);
    int status = esp_http_client_get_status_code(c);
    esp_http_client_cleanup(c);
    free(body);

    if (err != ESP_OK) {
        snprintf(reason, reason_len, "claim: could not reach the server (%s)",
                 esp_err_to_name(err));
        free(resp.buf);
        return ESP_FAIL;
    }
    if (status / 100 != 2) {
        /* 401 is an expired or already-used claim token, 409 is a device that
         * belongs to another household. The app turns both into plain
         * language; give it the words its regexes look for. */
        snprintf(reason, reason_len,
                 status == 401 ? "claim token expired or already used"
                               : "the server rejected the claim (HTTP %d)",
                 status);
        free(resp.buf);
        return ESP_FAIL;
    }

    cJSON *reply = cJSON_Parse(resp.buf);
    free(resp.buf);
    if (!reply) {
        snprintf(reason, reason_len, "claim: the server sent no usable reply");
        return ESP_FAIL;
    }
    const char *token = cJSON_GetStringValue(cJSON_GetObjectItem(reply, "device_token"));
    const char *name  = cJSON_GetStringValue(cJSON_GetObjectItem(reply, "device_name"));
    const char *house = cJSON_GetStringValue(cJSON_GetObjectItem(reply, "household_id"));
    if (!token || !token[0]) {
        cJSON_Delete(reply);
        snprintf(reason, reason_len, "claim: the server issued no device token");
        return ESP_FAIL;
    }

    /* Persist before reporting success: the app is about to tell the user the
     * mirror is paired, and a reboot between those two must not undo it. The
     * token is written last, because its presence is what "claimed" means. */
    esp_err_t store = mirror_config_set(MIRROR_CFG_SERVER_URL, s_server_url);
    if (store == ESP_OK && name) {
        store = mirror_config_set(MIRROR_CFG_DEVICE_NAME, name);
    }
    if (store == ESP_OK && house) {
        store = mirror_config_set(MIRROR_CFG_HOUSEHOLD_ID, house);
    }
    if (store == ESP_OK) {
        store = mirror_config_set(MIRROR_CFG_DEVICE_TOKEN, token);
    }
    if (store != ESP_OK) {
        cJSON_Delete(reply);
        snprintf(reason, reason_len, "claim: the device could not store its token");
        return store;
    }

    /* Keep the network the app just handed over, so the next boot rejoins it
     * without the provisioning manager. */
    esp_err_t adopted = mirror_net_adopt_sta_credentials();
    if (adopted != ESP_OK) {
        ESP_LOGW(TAG, "could not store the paired Wi-Fi credentials: %s",
                 esp_err_to_name(adopted));
    }

    report_claimed(name ? name : "Daily Mirror");
    ESP_LOGI(TAG, "claimed as \"%s\"", name ? name : "Daily Mirror");
    cJSON_Delete(reply);
    return ESP_OK;
}

/* ---------------------------------------------------------- the endpoint --- */

#if !CONFIG_BT_ENABLED

static esp_err_t start_service(void)
{
    ESP_LOGE(TAG, "this board has no Bluetooth radio - set it up through "
                  "/config instead");
    return ESP_ERR_NOT_SUPPORTED;
}

static void stop_service(void) { }

#else

/*
 * The "daily-mirror" endpoint. Two shapes of request, both from
 * mobile/src/pairing/provisioning.ts:
 *
 *   sendPayload()  a JSON ProvisioningPayload; the reply is the first
 *                  ProvisioningResult the app sees
 *   readResult()   "{}", polled every 1.5 s; the reply is whatever the flow
 *                  has got to
 *
 * The poll cannot send an empty body: protocomm hands the request to
 * security2's decrypt first, which fails on a zero-length payload
 * ("Failed to allocate decrypt buf len 0") and NimBLE then kills the
 * connection as "invalid content". So a poll is an empty JSON object, and any
 * object carrying neither field is read as one.
 *
 * protocomm frees *outbuf, so it must come from malloc.
 */
static esp_err_t pair_endpoint_handler(uint32_t session_id,
                                       const uint8_t *inbuf, ssize_t inlen,
                                       uint8_t **outbuf, ssize_t *outlen,
                                       void *priv_data)
{
    (void)session_id;
    (void)priv_data;

    if (inbuf && inlen > 0) {
        /* Copy first: the buffer is not NUL-terminated. */
        char *text = malloc((size_t)inlen + 1);
        if (!text) {
            return ESP_ERR_NO_MEM;
        }
        memcpy(text, inbuf, (size_t)inlen);
        text[inlen] = '\0';

        cJSON *doc = cJSON_Parse(text);
        free(text);
        const char *url = doc ? cJSON_GetStringValue(cJSON_GetObjectItem(doc, "server_url")) : NULL;
        const char *tok = doc ? cJSON_GetStringValue(cJSON_GetObjectItem(doc, "claim_token")) : NULL;

        if (doc && !url && !tok) {
            /* A poll: "{}", or any object with neither field. Nothing to do
             * but fall through to the reply below - in particular this must
             * not disturb a flow that is already waiting for the button. */
            cJSON_Delete(doc);
        } else if (!url || !tok || !url[0] || !tok[0]) {
            cJSON_Delete(doc);
            report_failed("the pairing payload was not understood");
        } else if (strlen(url) >= PAIR_FIELD_MAX || strlen(tok) >= PAIR_FIELD_MAX) {
            cJSON_Delete(doc);
            report_failed("the pairing payload was too large for this device");
        } else {
            lock();
            strlcpy(s_server_url, url, sizeof(s_server_url));
            strlcpy(s_claim_token, tok, sizeof(s_claim_token));
            unlock();
            cJSON_Delete(doc);

            /* Credentials received -> AwaitingConfirm. The window starts now,
             * not when the Wi-Fi credentials follow, because "press the button
             * now" is already on the user's screen. */
            s_confirmed = false;
            enter(MIRROR_PAIR_AWAITING_CONFIRM);
            mirror_ring_set(RING_AMBER_FAST_PULSE);
            report_awaiting_confirm();
            /* s_server_url, not `url`: the cJSON document that owned `url`
             * has just been freed. */
            ESP_LOGI(TAG, "claim payload received for %s - press the button to confirm",
                     s_server_url);
        }
    }

    lock();
    char *reply = strdup(s_result);
    unlock();
    if (!reply) {
        return ESP_ERR_NO_MEM;
    }
    *outbuf = (uint8_t *)reply;
    *outlen = (ssize_t)strlen(reply);
    return ESP_OK;
}

/* ------------------------------------------------------- the prov manager --- */

static void on_prov_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg;
    (void)base;

    switch (id) {
    case WIFI_PROV_START:
        ESP_LOGI(TAG, "pairing service \"%s\" is up", s_service_name);
        break;
    case WIFI_PROV_CRED_RECV: {
        wifi_sta_config_t *sta = data;
        ESP_LOGI(TAG, "Wi-Fi credentials received for \"%s\"", (const char *)sta->ssid);
        break;
    }
    case WIFI_PROV_CRED_FAIL: {
        wifi_prov_sta_fail_reason_t *reason = data;
        report_failed(*reason == WIFI_PROV_STA_AUTH_ERROR
                          ? "wifi password was not accepted"
                          : "wifi network was not found");
        /* Let the app try again on the same session rather than making the
         * user start the whole flow over. */
        wifi_prov_mgr_reset_sm_state_on_failure();
        break;
    }
    case WIFI_PROV_CRED_SUCCESS:
        ESP_LOGI(TAG, "joined the household network");
        break;
    case WIFI_PROV_DEINIT:
        /* FREE_BTDM has just handed the controller's memory back; it cannot be
         * taken again without a restart. */
        s_service_running = false;
        s_stopping = false;
        s_bt_released = true;
        ESP_LOGI(TAG, "pairing service down, Bluetooth memory released");
        break;
    default:
        break;
    }
}

/*
 * Salt and verifier for protocomm security 2 (SRP6a).
 *
 * Generated on the device rather than baked in, so the proof of possession is
 * a Kconfig string that can be changed in menuconfig without re-running
 * Espressif's esp_prov tooling to regenerate two byte arrays. It costs a few
 * hundred milliseconds once per pairing session.
 */
static esp_err_t ensure_srp_credentials(void)
{
    if (s_salt && s_verifier) {
        return ESP_OK;
    }
    const char *user = CONFIG_MIRROR_PROV_USERNAME;
    const char *pop  = CONFIG_MIRROR_PROV_POP;
    esp_err_t err = esp_srp_gen_salt_verifier(user, strlen(user), pop, strlen(pop),
                                              &s_salt, SRP_SALT_LEN,
                                              &s_verifier, &s_verifier_len);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "could not generate the security 2 salt/verifier: %s",
                 esp_err_to_name(err));
        s_salt = NULL;
        s_verifier = NULL;
    }
    return err;
}

/*
 * "mirror-<last 3 MAC bytes>", the same name the mDNS hostname and the bench
 * setup network use. The app scans for this prefix and the comparison is
 * case-sensitive, so it is lower case here and lower case there.
 *
 * Thirteen characters, comfortably inside the 29 the BLE advertisement allows
 * (MAX_BLE_DEVNAME_LEN) - there is no manufacturer data competing for the
 * scan response, so the whole name is advertised and the app can match on it.
 */
static void fill_service_name(void)
{
    char id[13];
    mirror_device_id(id);
    if (strcmp(id, "000000000000") == 0) {
        snprintf(s_service_name, sizeof(s_service_name), "mirror-setup");
    } else {
        snprintf(s_service_name, sizeof(s_service_name), "mirror-%s", id + 6);
    }
}

static esp_err_t start_service(void)
{
    if (s_service_running) {
        return ESP_OK;
    }

    esp_err_t err = ensure_srp_credentials();
    if (err != ESP_OK) {
        return err;
    }

    /* FREE_BTDM: hand the controller's memory back when provisioning ends.
     * The camera's frame buffers, the Wi-Fi stack and an mbedTLS handshake all
     * want internal RAM, and the BLE stack is only needed for the few minutes
     * of pairing. The cost is that re-pairing needs a restart, which
     * mirror_pair_start() handles.
     *
     * The service UUID is left at the provisioning default,
     * 0000ffff-0000-1000-8000-00805f9b34fb, because that is the one the
     * ESPProvision SDKs in the app already look for. */
    wifi_prov_mgr_config_t cfg = {
        .scheme = wifi_prov_scheme_ble,
        .scheme_event_handler = WIFI_PROV_SCHEME_BLE_EVENT_HANDLER_FREE_BTDM,
    };
    err = wifi_prov_mgr_init(cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "wifi_prov_mgr_init failed: %s", esp_err_to_name(err));
        return err;
    }
    s_service_running = true;

    /* The manager stops itself the moment Wi-Fi comes up, which would drop the
     * BLE link before the app has read the result of the claim - and the claim
     * has not even been attempted at that point, because it is still waiting
     * for the button. So: stop it ourselves, later. */
    wifi_prov_mgr_disable_auto_stop(1000);

    err = wifi_prov_mgr_endpoint_create(PAIR_ENDPOINT);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "endpoint create failed: %s", esp_err_to_name(err));
        wifi_prov_mgr_deinit();
        s_service_running = false;
        return err;
    }

    fill_service_name();
    wifi_prov_security2_params_t sec2 = {
        .salt = s_salt,
        .salt_len = SRP_SALT_LEN,
        .verifier = s_verifier,
        .verifier_len = (uint16_t)s_verifier_len,
    };
    /* No service key: BLE has no equivalent of a SoftAP password, and the
     * session is encrypted by security 2 regardless of the link. */
    err = wifi_prov_mgr_start_provisioning(WIFI_PROV_SECURITY_2, &sec2,
                                           s_service_name, NULL);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "wifi_prov_mgr_start_provisioning failed: %s", esp_err_to_name(err));
        wifi_prov_mgr_deinit();
        s_service_running = false;
        return err;
    }

    /* Registered after the start, as the manager's documentation requires. */
    err = wifi_prov_mgr_endpoint_register(PAIR_ENDPOINT, pair_endpoint_handler, NULL);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "endpoint register failed: %s", esp_err_to_name(err));
    }

    /* The manager forces WIFI_MODE_STA on start, which would take the bench
     * setup network - and /config with it - off the air for the whole pairing
     * window. BLE does not care what the Wi-Fi mode is. */
    mirror_net_restore_ap_mode();

    ESP_LOGW(TAG, "pairing over BLE as \"%s\" - open \"Add a mirror\" in the app "
                  "(security 2, user \"%s\", pop \"%s\")",
             s_service_name, CONFIG_MIRROR_PROV_USERNAME, CONFIG_MIRROR_PROV_POP);
    return ESP_OK;
}

static void stop_service(void)
{
    if (!s_service_running) {
        return;
    }
    ESP_LOGI(TAG, "stopping the pairing service");
    s_stopping = true;
    wifi_prov_mgr_stop_provisioning();
    /* WIFI_PROV_DEINIT clears s_service_running once the cleanup delay is up. */
}

#endif /* CONFIG_BT_ENABLED */

/* ----------------------------------------------------------- the machine --- */

ring_pattern_t mirror_pair_ring(void)
{
    switch (s_state) {
    case MIRROR_PAIR_PAIRING:
        return RING_AMBER_CHASE;
    case MIRROR_PAIR_AWAITING_CONFIRM:
        return RING_AMBER_FAST_PULSE;
    case MIRROR_PAIR_CLAIMING:
        return RING_BLUE_SLOW_PULSE;
    case MIRROR_PAIR_IDLE:
    default:
        /* Claimed is Ready; unclaimed is the offline breathe - the device
         * still captures, it is just not paired with anything. */
        return mirror_pair_claimed() ? RING_SOLID_WHITE : RING_DIM_WHITE_BREATHE;
    }
}

static void back_to_pairing(const char *reason)
{
    report_failed(reason);
    mirror_ring_play(RING_RED_TRIPLE_PULSE, RING_AMBER_CHASE);
    s_confirmed = false;
    s_claim_token[0] = '\0';
    enter(MIRROR_PAIR_PAIRING);
}

static void do_claim(void)
{
    enter(MIRROR_PAIR_CLAIMING);
    mirror_ring_set(RING_BLUE_SLOW_PULSE);

    /* The manager does the join as part of the standard wifi_config exchange,
     * so by now it is usually already up; this is the tail of a slow DHCP. */
    int64_t deadline = now_ms() + JOIN_WAIT_MS;
    while (!mirror_net_sta_connected() && now_ms() < deadline) {
        vTaskDelay(pdMS_TO_TICKS(TICK_MS));
    }
    if (!mirror_net_sta_connected()) {
        back_to_pairing("wifi join failed - no address");
        return;
    }

    char reason[160];
    if (redeem_claim(reason, sizeof(reason)) != ESP_OK) {
        back_to_pairing(reason);
        return;
    }

    /* Ready. The mDNS record is what the app uses to find the device on the
     * household network from here on. */
    mirror_mdns_set_claimed(true);
    mirror_ring_set(RING_SOLID_WHITE);
    enter(MIRROR_PAIR_IDLE);
}

static void pair_task(void *arg)
{
    (void)arg;

    for (;;) {
        switch (s_state) {
        case MIRROR_PAIR_PAIRING:
            if (in_state_ms() >= PAIRING_TIMEOUT_MS) {
                ESP_LOGW(TAG, "nothing paired in %d minutes - back to offline capture",
                         PAIRING_TIMEOUT_MS / 60000);
                enter(MIRROR_PAIR_IDLE);
                stop_service();
                mirror_ring_set(mirror_pair_ring());
                s_task = NULL;
                vTaskDelete(NULL);
                return;
            }
            break;

        case MIRROR_PAIR_AWAITING_CONFIRM:
            if (s_confirmed) {
                s_confirmed = false;
                do_claim();
            } else if (in_state_ms() >= CONFIRM_TIMEOUT_MS) {
                /* No press, no claim. Back to discoverable so the app can
                 * simply try again. */
                report_failed("not confirmed - the button was not pressed");
                mirror_ring_set(RING_AMBER_CHASE);
                s_claim_token[0] = '\0';
                enter(MIRROR_PAIR_PAIRING);
            }
            break;

        case MIRROR_PAIR_IDLE:
            /* Claimed, and lingering so the phone can read the result over a
             * network that is about to go away. */
            if (in_state_ms() >= CLAIMED_LINGER_MS) {
                stop_service();
                s_task = NULL;
                vTaskDelete(NULL);
                return;
            }
            break;

        case MIRROR_PAIR_CLAIMING:
        default:
            break;
        }
        vTaskDelay(pdMS_TO_TICKS(TICK_MS));
    }
}

/* -------------------------------------------------------------- the API --- */

bool mirror_pair_claimed(void)
{
    return mirror_config_get(MIRROR_CFG_DEVICE_TOKEN)[0] != '\0';
}

mirror_pair_state_t mirror_pair_state(void) { return s_state; }

bool mirror_pair_busy(void)
{
    return s_state != MIRROR_PAIR_IDLE;
}

bool mirror_pair_confirm(void)
{
    if (s_state == MIRROR_PAIR_AWAITING_CONFIRM) {
        ESP_LOGI(TAG, "confirmed by the button");
        s_confirmed = true;
        return true;
    }
    /* Pairing and claiming swallow the press too: the plan gives the button to
     * pairing while pairing is happening, so a press cannot start a capture
     * from under it. */
    return s_state != MIRROR_PAIR_IDLE;
}

bool mirror_pair_wanted_at_boot(void)
{
    bool armed = mirror_config_get(MIRROR_CFG_PAIR_ON_BOOT)[0] != '\0';
    if (armed) {
        /* One shot: a restart for any other reason must not land back here. */
        mirror_config_set(MIRROR_CFG_PAIR_ON_BOOT, "");
        ESP_LOGI(TAG, "a long-press before the last restart asked for pairing");
    }
    return armed || !mirror_pair_claimed();
}

esp_err_t mirror_pair_start(void)
{
    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutex();
        if (s_lock == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

    /* A long press that lands in the second between "stop the service" and the
     * manager finishing would otherwise re-enter pairing on a service that is
     * on its way down. Wait for it to finish and take the restart path. */
    for (int i = 0; s_stopping && i < 25; i++) {
        vTaskDelay(pdMS_TO_TICKS(200));
    }

    if (s_bt_released) {
        /* The BLE stack is gone and only a restart brings it back. Arm pairing
         * for the next boot and go: the stored credentials and the device
         * token are untouched, so this is safe even from Ready. */
        ESP_LOGW(TAG, "re-pairing needs the Bluetooth stack back - restarting into pairing");
        mirror_ring_set(RING_AMBER_CHASE);
        mirror_config_set(MIRROR_CFG_PAIR_ON_BOOT, "1");
        vTaskDelay(pdMS_TO_TICKS(250));
        esp_restart();
        return ESP_OK;
    }

    /* Re-entering pairing restarts the window and clears any half-finished
     * attempt, but never touches the stored credentials: they survive until a
     * new claim succeeds. */
    s_confirmed = false;
    s_claim_token[0] = '\0';
    report_awaiting_confirm();
    enter(MIRROR_PAIR_PAIRING);

    esp_err_t err;
#if CONFIG_BT_ENABLED
    err = esp_event_handler_register(WIFI_PROV_EVENT, ESP_EVENT_ANY_ID,
                                     &on_prov_event, NULL);
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "prov event handler: %s", esp_err_to_name(err));
    }
#endif

    err = start_service();
    if (err != ESP_OK) {
        enter(MIRROR_PAIR_IDLE);
        mirror_ring_set(mirror_pair_ring());
        return err;
    }

    mirror_ring_set(RING_AMBER_CHASE);

    if (s_task == NULL) {
        /* 8 KB: the claim runs on this task and mbedTLS' handshake is most of
         * it, the same reason the button task is 8 KB. */
        if (xTaskCreate(pair_task, "pair", 8192, NULL, 5, &s_task) != pdPASS) {
            s_task = NULL;
            return ESP_ERR_NO_MEM;
        }
    }
    return ESP_OK;
}

void mirror_pair_factory_reset(void)
{
    ESP_LOGW(TAG, "full reset - erasing all device state");
    mirror_ring_set(RING_OFF);
    mirror_config_erase_all();
    /* The Wi-Fi driver keeps its own copy of the credentials in nvs.net80211,
     * which the provisioning manager wrote; erase that too or the device would
     * rejoin the old network on the next boot. This is all
     * wifi_prov_mgr_reset_provisioning() does, and unlike that function it
     * exists on boards without the provisioning component. */
    esp_wifi_restore();
    vTaskDelay(pdMS_TO_TICKS(250));
    esp_restart();
}
