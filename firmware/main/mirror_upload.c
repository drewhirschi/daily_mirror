#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include "mirror_upload.h"
#include "mirror_config.h"
#include "mirror_mdns.h"

#include "esp_log.h"
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "esp_timer.h"
#include "cJSON.h"

static const char *TAG = "mirror_upload";

static char s_last[160] = "none yet";

typedef struct {
    char *buf;
    size_t len;
} resp_buf_t;

static esp_err_t collect(esp_http_client_event_t *evt)
{
    resp_buf_t *r = evt->user_data;
    if (evt->event_id == HTTP_EVENT_ON_DATA && r) {
        /* Cap the reply: a grant is a few hundred bytes, and an unbounded
         * realloc loop on a misconfigured URL would eat the heap. */
        if (r->len + evt->data_len > 4096) {
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

bool mirror_upload_configured(void)
{
    return mirror_config_get(MIRROR_CFG_SERVER_URL)[0] != '\0'
        && mirror_config_get(MIRROR_CFG_UPLOAD_TOKEN)[0] != '\0';
}

const char *mirror_upload_last_result(void) { return s_last; }

/* Join a possibly-relative URL from a grant onto the server origin. */
static void resolve(char *out, size_t out_len, const char *base, const char *ref)
{
    if (strncmp(ref, "http", 4) == 0) {
        snprintf(out, out_len, "%s", ref);
    } else {
        snprintf(out, out_len, "%s%s%s", base, ref[0] == '/' ? "" : "/", ref);
    }
}

esp_err_t mirror_upload_jpeg(const uint8_t *jpeg, size_t len)
{
    const char *server = mirror_config_get(MIRROR_CFG_SERVER_URL);
    const char *token = mirror_config_get(MIRROR_CFG_UPLOAD_TOKEN);
    if (!server[0]) {
        snprintf(s_last, sizeof(s_last), "no server_url configured");
        return ESP_ERR_INVALID_STATE;
    }

    /* capture_id: the stable device id plus the seconds since boot. Unique per
     * device per press, and the server can tell which device it came from
     * without a lookup. */
    char device_id[13];
    mirror_device_id(device_id);
    char capture_id[48];
    snprintf(capture_id, sizeof(capture_id), "%s-%" PRIu64, device_id,
             (uint64_t)(esp_timer_get_time() / 1000000));

    char auth[MIRROR_CONFIG_VALUE_MAX + 8];
    snprintf(auth, sizeof(auth), "Bearer %s", token);

    /* ---- 1. grant ---- */
    char url[MIRROR_CONFIG_VALUE_MAX + 32];
    snprintf(url, sizeof(url), "%s/api/uploads", server);
    char body[192];
    snprintf(body, sizeof(body),
             "{\"capture_id\":\"%s\",\"content_type\":\"image/jpeg\",\"content_length\":%u}",
             capture_id, (unsigned)len);

    resp_buf_t resp = { 0 };
    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST,
        .event_handler = collect, .user_data = &resp,
        .timeout_ms = 15000,
        /* The certificate bundle, not a disabled check. The bench build used
         * CONFIG_ESP_TLS_SKIP_SERVER_CERT_VERIFY, which makes every HTTPS
         * upload trivially interceptable; this verifies against the Mozilla
         * root set compiled into the image. */
        .crt_bundle_attach = esp_crt_bundle_attach,
    };
    esp_http_client_handle_t c = esp_http_client_init(&cfg);
    if (!c) {
        snprintf(s_last, sizeof(s_last), "grant: client init failed");
        return ESP_FAIL;
    }
    esp_http_client_set_header(c, "Content-Type", "application/json");
    if (token[0]) {
        esp_http_client_set_header(c, "Authorization", auth);
    }
    esp_http_client_set_post_field(c, body, strlen(body));
    esp_err_t err = esp_http_client_perform(c);
    int status = esp_http_client_get_status_code(c);
    esp_http_client_cleanup(c);
    if (err != ESP_OK || status / 100 != 2) {
        snprintf(s_last, sizeof(s_last), "grant failed: %s HTTP %d %.60s",
                 esp_err_to_name(err), status, resp.buf ? resp.buf : "");
        free(resp.buf);
        return ESP_FAIL;
    }

    cJSON *g = cJSON_Parse(resp.buf);
    free(resp.buf);
    if (!g) {
        snprintf(s_last, sizeof(s_last), "grant: bad json");
        return ESP_FAIL;
    }
    const char *gurl = cJSON_GetStringValue(cJSON_GetObjectItem(g, "url"));
    const char *gmethod = cJSON_GetStringValue(cJSON_GetObjectItem(g, "method"));
    const char *gcomplete = cJSON_GetStringValue(cJSON_GetObjectItem(g, "complete_url"));
    cJSON *gheaders = cJSON_GetObjectItem(g, "headers");
    if (!gurl) {
        snprintf(s_last, sizeof(s_last), "grant: no url");
        cJSON_Delete(g);
        return ESP_FAIL;
    }

    char target[640];
    resolve(target, sizeof(target), server, gurl);
    bool same_origin = strncmp(target, server, strlen(server)) == 0;

    /* ---- 2. the bytes ---- */
    esp_http_client_config_t ucfg = {
        .url = target, .timeout_ms = 60000,
        .method = (gmethod && strcmp(gmethod, "POST") == 0) ? HTTP_METHOD_POST : HTTP_METHOD_PUT,
        .crt_bundle_attach = esp_crt_bundle_attach,
    };
    esp_http_client_handle_t u = esp_http_client_init(&ucfg);
    if (!u) {
        cJSON_Delete(g);
        snprintf(s_last, sizeof(s_last), "put: client init failed");
        return ESP_FAIL;
    }
    esp_http_client_set_header(u, "Content-Type", "image/jpeg");
    cJSON *h;
    cJSON_ArrayForEach(h, gheaders) {
        if (cJSON_IsString(h)) {
            esp_http_client_set_header(u, h->string, h->valuestring);
        }
    }
    /* Only same-origin. A grant that points at object storage carries its own
     * signature in the URL and must never see our bearer token. */
    if (same_origin && token[0]) {
        esp_http_client_set_header(u, "Authorization", auth);
    }
    esp_http_client_set_post_field(u, (const char *)jpeg, len);
    err = esp_http_client_perform(u);
    status = esp_http_client_get_status_code(u);
    esp_http_client_cleanup(u);
    if (err != ESP_OK || status / 100 != 2) {
        snprintf(s_last, sizeof(s_last), "put failed: %s HTTP %d", esp_err_to_name(err), status);
        cJSON_Delete(g);
        return ESP_FAIL;
    }

    /* ---- 3. complete ---- */
    if (gcomplete) {
        char curl[640];
        resolve(curl, sizeof(curl), server, gcomplete);
        esp_http_client_config_t ccfg = {
            .url = curl, .method = HTTP_METHOD_POST, .timeout_ms = 15000,
            .crt_bundle_attach = esp_crt_bundle_attach,
        };
        esp_http_client_handle_t cc = esp_http_client_init(&ccfg);
        if (!cc) {
            cJSON_Delete(g);
            snprintf(s_last, sizeof(s_last), "complete: client init failed");
            return ESP_FAIL;
        }
        if (token[0]) {
            esp_http_client_set_header(cc, "Authorization", auth);
        }
        esp_http_client_set_post_field(cc, "", 0);
        err = esp_http_client_perform(cc);
        status = esp_http_client_get_status_code(cc);
        esp_http_client_cleanup(cc);
        if (err != ESP_OK || status / 100 != 2) {
            snprintf(s_last, sizeof(s_last), "complete failed: %s HTTP %d",
                     esp_err_to_name(err), status);
            cJSON_Delete(g);
            return ESP_FAIL;
        }
    }

    cJSON_Delete(g);
    snprintf(s_last, sizeof(s_last), "ok: %s (%u bytes)", capture_id, (unsigned)len);
    ESP_LOGI(TAG, "upload %s", s_last);
    return ESP_OK;
}
