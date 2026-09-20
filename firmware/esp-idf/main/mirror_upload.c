#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

#include <time.h>

#include "mirror_upload.h"
#include "mirror_config.h"
#include "mirror_mdns.h"
#include "mirror_net.h"

#include "esp_log.h"
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "cJSON.h"

static const char *TAG = "mirror_upload";

static char s_last[160] = "none yet";
static volatile bool s_blocked;

/* 8 KB out of PSRAM per upload. The internal heap is the scarce one here - the
 * low-water mark across a TLS upload is around 73 KB - so the staging buffer
 * goes where there are megabytes spare. */
#define CHUNK 8192

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

/*
 * The bearer the server will accept.
 *
 * The per-device token a claim issued wins: it is what the devices table looks
 * up, and it is what records which household a photo belongs to. The shared
 * upload token is the Pi-era credential and stays only as a fallback for a
 * bench unit configured through /config, which is why the plan calls it
 * retired rather than gone.
 */
static const char *bearer_token(void)
{
    const char *device_token = mirror_config_get(MIRROR_CFG_DEVICE_TOKEN);
    return device_token[0] != '\0' ? device_token
                                   : mirror_config_get(MIRROR_CFG_UPLOAD_TOKEN);
}

bool mirror_upload_configured(void)
{
    return mirror_config_get(MIRROR_CFG_SERVER_URL)[0] != '\0'
        && bearer_token()[0] != '\0';
}

const char *mirror_upload_last_result(void) { return s_last; }

void mirror_upload_set_blocked(bool blocked)
{
    s_blocked = blocked;
    ESP_LOGW(TAG, "debug upload block %s", blocked ? "ON - every upload will fail" : "off");
}

bool mirror_upload_blocked(void) { return s_blocked; }

/* Join a possibly-relative URL from a grant onto the server origin. */
static void resolve(char *out, size_t out_len, const char *base, const char *ref)
{
    if (strncmp(ref, "http", 4) == 0) {
        snprintf(out, out_len, "%s", ref);
    } else {
        snprintf(out, out_len, "%s%s%s", base, ref[0] == '/' ? "" : "/", ref);
    }
}

/*
 * A URL with its query string cut off.
 *
 * Presigned object-storage URLs carry the signature, the expiry and the access
 * key id as query parameters. Logging one - at any level, to the console or to
 * the ring /logs serves over the LAN - hands out a working write credential
 * for that object. Nothing in this file prints a URL except through here.
 */
static void redact_url(char *out, size_t out_len, const char *url)
{
    const char *q = strchr(url, '?');
    if (!q) {
        snprintf(out, out_len, "%s", url);
        return;
    }
    int keep = (int)(q - url);
    snprintf(out, out_len, "%.*s?<redacted>", keep, url);
}

/* Send `len` bytes of `f` to an already-configured client, in CHUNK pieces. */
static esp_err_t stream_body(esp_http_client_handle_t u, FILE *f, size_t len,
                             char *chunk, int *status_out)
{
    esp_err_t err = esp_http_client_open(u, (int)len);
    if (err != ESP_OK) {
        return err;
    }
    size_t sent = 0;
    while (sent < len) {
        size_t want = len - sent;
        if (want > CHUNK) {
            want = CHUNK;
        }
        size_t got = fread(chunk, 1, want, f);
        if (got == 0) {
            /* The file is shorter than its header claims: a half-written spool
             * entry that survived a power cut mid-rename. */
            return ESP_ERR_INVALID_SIZE;
        }
        int w = esp_http_client_write(u, chunk, got);
        if (w < 0) {
            return ESP_FAIL;
        }
        sent += (size_t)w;
        if ((size_t)w < got && fseek(f, (long)w - (long)got, SEEK_CUR) != 0) {
            return ESP_FAIL;
        }
    }
    if (esp_http_client_fetch_headers(u) < 0) {
        return ESP_FAIL;
    }
    *status_out = esp_http_client_get_status_code(u);
    return ESP_OK;
}

esp_err_t mirror_upload_file(const mirror_upload_req_t *req, int *http_status)
{
    if (http_status) {
        *http_status = 0;
    }
    if (!req || !req->f || !req->capture_id) {
        return ESP_ERR_INVALID_ARG;
    }

    const char *server = mirror_config_get(MIRROR_CFG_SERVER_URL);
    const char *token = bearer_token();
    if (!server[0]) {
        snprintf(s_last, sizeof(s_last), "no server_url configured");
        return ESP_ERR_INVALID_STATE;
    }
    if (s_blocked) {
        snprintf(s_last, sizeof(s_last), "blocked by the debug upload block");
        return ESP_FAIL;
    }

    char auth[MIRROR_CONFIG_VALUE_MAX + 8];
    snprintf(auth, sizeof(auth), "Bearer %s", token);

    /* ---- 1. grant ---- */
    char url[MIRROR_CONFIG_VALUE_MAX + 32];
    snprintf(url, sizeof(url), "%s/api/uploads", server);

    /* The body carries the provenance object when the board could read one.
     * The server ignores fields it does not know, so firmware and server can
     * ship in either order. */
    size_t body_cap = 256 + (req->meta_json ? strlen(req->meta_json) : 0);
    char *body = malloc(body_cap);
    if (!body) {
        snprintf(s_last, sizeof(s_last), "grant: no memory");
        return ESP_ERR_NO_MEM;
    }
    int bn = snprintf(body, body_cap,
             "{\"capture_id\":\"%s\",\"content_type\":\"image/jpeg\",\"content_length\":%u",
             req->capture_id, (unsigned)req->len);
    if (req->meta_json && req->meta_json[0]) {
        bn += snprintf(body + bn, body_cap - bn, ",\"capture\":%s", req->meta_json);
    }
    snprintf(body + bn, body_cap - bn, "}");
    /* Debug level, and it contains no credential: capture_id, sizes and sensor
     * readings only. The bearer token is set as a header, never in the body. */
    ESP_LOGD(TAG, "grant body %s", body);

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
        free(body);
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
    free(body);
    if (http_status) {
        *http_status = status;
    }
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

    char safe[128];
    redact_url(safe, sizeof(safe), target);
    ESP_LOGD(TAG, "put %u bytes -> %s", (unsigned)req->len, safe);

    /* ---- 2. the bytes, streamed ---- */
    char *chunk = heap_caps_malloc(CHUNK, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!chunk) {
        cJSON_Delete(g);
        snprintf(s_last, sizeof(s_last), "put: no PSRAM for the staging buffer");
        return ESP_ERR_NO_MEM;
    }

    esp_http_client_config_t ucfg = {
        .url = target, .timeout_ms = 60000,
        .method = (gmethod && strcmp(gmethod, "POST") == 0) ? HTTP_METHOD_POST : HTTP_METHOD_PUT,
        .crt_bundle_attach = esp_crt_bundle_attach,
    };
    esp_http_client_handle_t u = esp_http_client_init(&ucfg);
    if (!u) {
        free(chunk);
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

    status = 0;
    err = stream_body(u, req->f, req->len, chunk, &status);
    esp_http_client_close(u);
    esp_http_client_cleanup(u);
    free(chunk);
    if (http_status) {
        *http_status = status;
    }
    if (err != ESP_OK || status / 100 != 2) {
        snprintf(s_last, sizeof(s_last), "put failed: %s HTTP %d",
                 esp_err_to_name(err), status);
        cJSON_Delete(g);
        return ESP_FAIL;
    }

    ESP_LOGI(TAG, "put ok: HTTP %d, %u bytes", status, (unsigned)req->len);

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
        if (http_status) {
            *http_status = status;
        }
        if (err != ESP_OK || status / 100 != 2) {
            snprintf(s_last, sizeof(s_last), "complete failed: %s HTTP %d",
                     esp_err_to_name(err), status);
            cJSON_Delete(g);
            return ESP_FAIL;
        }
    }

    cJSON_Delete(g);
    snprintf(s_last, sizeof(s_last), "ok: %s (%u bytes)",
             req->capture_id, (unsigned)req->len);
    ESP_LOGI(TAG, "upload %s", s_last);
    return ESP_OK;
}
