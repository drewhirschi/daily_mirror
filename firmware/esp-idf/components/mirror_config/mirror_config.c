/*
 * Daily Mirror device settings: NVS-backed store plus the admin settings page.
 * See include/mirror_config.h for the contract and README.md for the routes.
 */

#include "mirror_config.h"
#include "mirror_form.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "nvs.h"
#include "nvs_flash.h"
#include "sdkconfig.h"

#define MIRROR_NVS_NAMESPACE "mirror"

/* Anything larger than this is a mistake or an attack, not a settings form. */
#define MIRROR_FORM_BODY_MAX 2048

/* Seconds to wait before rebooting so the HTTP response actually flushes. */
#define MIRROR_REBOOT_DELAY_MS 1000

static const char *TAG = "mirror_cfg";

/* Kconfig defaults. Empty unless a (git-ignored) sdkconfig sets them. */
#ifndef CONFIG_MIRROR_DEFAULT_WIFI_SSID
#define CONFIG_MIRROR_DEFAULT_WIFI_SSID ""
#endif
#ifndef CONFIG_MIRROR_DEFAULT_WIFI_PASS
#define CONFIG_MIRROR_DEFAULT_WIFI_PASS ""
#endif
#ifndef CONFIG_MIRROR_DEFAULT_SERVER_URL
#define CONFIG_MIRROR_DEFAULT_SERVER_URL ""
#endif
#ifndef CONFIG_MIRROR_DEFAULT_UPLOAD_TOKEN
#define CONFIG_MIRROR_DEFAULT_UPLOAD_TOKEN ""
#endif

/* ---------------------------------------------------------------- cache --- */

typedef struct {
    const char *key;
    const char *build_default;
    char        value[MIRROR_CONFIG_VALUE_MAX];
    bool        from_nvs;
} mirror_setting_t;

/* Order is the order the settings page renders them in. */
static mirror_setting_t s_settings[] = {
    { MIRROR_CFG_WIFI_SSID,    CONFIG_MIRROR_DEFAULT_WIFI_SSID,    {0}, false },
    { MIRROR_CFG_WIFI_PASS,    CONFIG_MIRROR_DEFAULT_WIFI_PASS,    {0}, false },
    { MIRROR_CFG_SERVER_URL,   CONFIG_MIRROR_DEFAULT_SERVER_URL,   {0}, false },
    { MIRROR_CFG_UPLOAD_TOKEN, CONFIG_MIRROR_DEFAULT_UPLOAD_TOKEN, {0}, false },
    { MIRROR_CFG_DEVICE_NAME,  "",                                 {0}, false },
};

#define MIRROR_SETTING_COUNT (sizeof(s_settings) / sizeof(s_settings[0]))

static SemaphoreHandle_t s_lock;
static StaticSemaphore_t s_lock_buf;
static bool s_inited;

static mirror_setting_t *find_setting(const char *key)
{
    if (key == NULL) return NULL;
    for (size_t i = 0; i < MIRROR_SETTING_COUNT; i++) {
        if (strcmp(s_settings[i].key, key) == 0) return &s_settings[i];
    }
    return NULL;
}

static void lock(void)
{
    if (s_lock != NULL) xSemaphoreTake(s_lock, portMAX_DELAY);
}

static void unlock(void)
{
    if (s_lock != NULL) xSemaphoreGive(s_lock);
}

/* Caller holds the lock. */
static void load_all_locked(void)
{
    nvs_handle_t h;
    esp_err_t err = nvs_open(MIRROR_NVS_NAMESPACE, NVS_READONLY, &h);
    if (err != ESP_OK) {
        /* No namespace yet: everything falls back to the build-time defaults. */
        ESP_LOGI(TAG, "no stored settings yet (%s)", esp_err_to_name(err));
        for (size_t i = 0; i < MIRROR_SETTING_COUNT; i++) {
            s_settings[i].value[0] = '\0';
            s_settings[i].from_nvs = false;
        }
        return;
    }
    for (size_t i = 0; i < MIRROR_SETTING_COUNT; i++) {
        size_t len = sizeof(s_settings[i].value);
        err = nvs_get_str(h, s_settings[i].key, s_settings[i].value, &len);
        if (err == ESP_OK) {
            s_settings[i].from_nvs = true;
        } else {
            s_settings[i].value[0] = '\0';
            s_settings[i].from_nvs = false;
        }
    }
    nvs_close(h);
}

esp_err_t mirror_config_init(void)
{
    if (s_inited) return ESP_OK;

    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutexStatic(&s_lock_buf);
    }

    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_LOGW(TAG, "erasing NVS (%s)", esp_err_to_name(err));
        ESP_ERROR_CHECK(nvs_flash_erase());
        err = nvs_flash_init();
    }
    if (err == ESP_ERR_INVALID_STATE) {
        /* Already initialised by the application: that is fine. */
        err = ESP_OK;
    }
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "nvs_flash_init failed: %s", esp_err_to_name(err));
        return err;
    }

    lock();
    load_all_locked();
    s_inited = true;
    unlock();

    ESP_LOGI(TAG, "settings loaded (wifi %s)",
             mirror_config_has_wifi() ? "configured" : "unset");
    return ESP_OK;
}

const char *mirror_config_get(const char *key)
{
    mirror_setting_t *s = find_setting(key);
    if (s == NULL) return "";

    /* The returned pointer is into the long-lived cache, so it stays valid
     * until the next set() for this key, as the header promises. */
    lock();
    const char *out = s->from_nvs ? s->value : s->build_default;
    unlock();
    return out != NULL ? out : "";
}

esp_err_t mirror_config_set(const char *key, const char *value)
{
    mirror_setting_t *s = find_setting(key);
    if (s == NULL) return ESP_ERR_INVALID_ARG;
    if (value == NULL) value = "";
    if (strlen(value) >= MIRROR_CONFIG_VALUE_MAX) return ESP_ERR_INVALID_SIZE;

    nvs_handle_t h;
    esp_err_t err = nvs_open(MIRROR_NVS_NAMESPACE, NVS_READWRITE, &h);
    if (err != ESP_OK) return err;

    if (value[0] == '\0') {
        err = nvs_erase_key(h, key);
        if (err == ESP_ERR_NVS_NOT_FOUND) err = ESP_OK;
    } else {
        err = nvs_set_str(h, key, value);
    }
    if (err == ESP_OK) err = nvs_commit(h); /* commit before the cache moves */
    nvs_close(h);
    if (err != ESP_OK) return err;

    lock();
    if (value[0] == '\0') {
        s->value[0] = '\0';
        s->from_nvs = false;
    } else {
        strlcpy(s->value, value, sizeof(s->value));
        s->from_nvs = true;
    }
    unlock();
    return ESP_OK;
}

bool mirror_config_has_wifi(void)
{
    return mirror_config_get(MIRROR_CFG_WIFI_SSID)[0] != '\0';
}

esp_err_t mirror_config_erase_all(void)
{
    nvs_handle_t h;
    esp_err_t err = nvs_open(MIRROR_NVS_NAMESPACE, NVS_READWRITE, &h);
    if (err == ESP_ERR_NVS_NOT_FOUND) {
        err = ESP_OK; /* nothing stored: already erased */
    } else if (err == ESP_OK) {
        err = nvs_erase_all(h);
        if (err == ESP_OK) err = nvs_commit(h);
        nvs_close(h);
    }
    if (err != ESP_OK) return err;

    lock();
    for (size_t i = 0; i < MIRROR_SETTING_COUNT; i++) {
        s_settings[i].value[0] = '\0';
        s_settings[i].from_nvs = false;
    }
    unlock();
    ESP_LOGW(TAG, "all settings erased");
    return ESP_OK;
}

/* --------------------------------------------------------------- reboot --- */

static void reboot_task(void *arg)
{
    (void)arg;
    vTaskDelay(pdMS_TO_TICKS(MIRROR_REBOOT_DELAY_MS));
    ESP_LOGW(TAG, "restarting to apply settings");
    esp_restart();
}

/* Give the HTTP response time to reach the browser before we restart. */
static void reboot_soon(void)
{
    static bool pending;
    if (pending) return;
    pending = true;
    if (xTaskCreate(reboot_task, "cfg_reboot", 2560, NULL, 5, NULL) != pdPASS) {
        ESP_LOGE(TAG, "reboot task failed; restarting now");
        esp_restart();
    }
}

/* ----------------------------------------------------------------- HTTP --- */

static esp_err_t send_error(httpd_req_t *req, const char *msg)
{
    char page[512];
    char safe[256];
    mirror_html_escape(msg, safe, sizeof(safe));
    int n = snprintf(page, sizeof(page),
                     "<!doctype html><meta name=viewport content=\"width=device-width,"
                     "initial-scale=1\"><body style=\"background:#111;color:#eee;"
                     "font:15px system-ui;padding:24px\"><h2>Not saved</h2><p>%s</p>"
                     "<p><a style=\"color:#8cf\" href=\"/config\">Back to settings</a></p>",
                     safe);
    httpd_resp_set_status(req, "400 Bad Request");
    httpd_resp_set_type(req, "text/html");
    return httpd_resp_send(req, page, n > 0 ? n : 0);
}

static esp_err_t send_rebooting(httpd_req_t *req, const char *what)
{
    char page[512];
    int n = snprintf(page, sizeof(page),
                     "<!doctype html><meta name=viewport content=\"width=device-width,"
                     "initial-scale=1\"><body style=\"background:#111;color:#eee;"
                     "font:15px system-ui;padding:24px\"><h2>%s</h2>"
                     "<p>The camera is restarting and will rejoin Wi-Fi in a few "
                     "seconds. If the network changed, look for it at its new "
                     "address.</p>", what);
    httpd_resp_set_type(req, "text/html");
    esp_err_t err = httpd_resp_send(req, page, n > 0 ? n : 0);
    reboot_soon();
    return err;
}

/* Append to a growing buffer, tracking overflow instead of truncating silently. */
typedef struct {
    char  *buf;
    size_t size;
    size_t len;
} sbuf_t;

static void sb_printf(sbuf_t *sb, const char *fmt, ...) __attribute__((format(printf, 2, 3)));

static void sb_printf(sbuf_t *sb, const char *fmt, ...)
{
    if (sb->len >= sb->size) return;
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(sb->buf + sb->len, sb->size - sb->len, fmt, ap);
    va_end(ap);
    if (n > 0) {
        sb->len += ((size_t)n < sb->size - sb->len) ? (size_t)n : (sb->size - sb->len);
    }
}

/* A plain text field: its current value is safe to render (escaped). */
static void render_text_field(sbuf_t *sb, const char *id, const char *label,
                              const char *value, const char *hint)
{
    /* Static: worst-case escaping is 6x the value, too much for the httpd
     * task stack, and only the single httpd task ever renders this page. */
    static char safe[MIRROR_CONFIG_VALUE_MAX * 6];
    mirror_html_escape(value, safe, sizeof(safe));
    sb_printf(sb,
              "<label for=\"%s\">%s</label>"
              "<input id=\"%s\" name=\"%s\" value=\"%s\" maxlength=\"%d\" "
              "autocapitalize=off autocorrect=off spellcheck=false>"
              "<p class=h>%s</p>",
              id, label, id, id, safe, MIRROR_CONFIG_VALUE_MAX - 1, hint);
}

/* A secret field: never echo the stored value, only whether one exists. */
static void render_secret_field(sbuf_t *sb, const char *id, const char *label,
                                bool is_set)
{
    sb_printf(sb,
              "<label for=\"%s\">%s</label>"
              "<input id=\"%s\" name=\"%s\" type=password maxlength=\"%d\" "
              "placeholder=\"%s\" autocapitalize=off autocorrect=off spellcheck=false>"
              "<p class=h>%s</p>",
              id, label, id, id, MIRROR_CONFIG_VALUE_MAX - 1,
              is_set ? "\xe2\x80\xa2\xe2\x80\xa2\xe2\x80\xa2\xe2\x80\xa2 set" : "not set",
              is_set ? "Stored. Leave blank to keep it unchanged."
                     : "Nothing stored yet.");
}

static esp_err_t get_config_handler(httpd_req_t *req)
{
    /* Roughly 3 KB of chrome plus the escaped values. */
    static char page[6144];
    sbuf_t sb = { page, sizeof(page), 0 };

    bool pass_set  = mirror_config_get(MIRROR_CFG_WIFI_PASS)[0]    != '\0';
    bool token_set = mirror_config_get(MIRROR_CFG_UPLOAD_TOKEN)[0] != '\0';

    sb_printf(&sb,
              "<!doctype html><html><head><meta charset=utf-8>"
              "<meta name=viewport content=\"width=device-width,initial-scale=1\">"
              "<title>Daily Mirror settings</title><style>"
              "body{background:#111;color:#eee;font:15px system-ui;margin:0;"
              "padding:24px 16px;max-width:30rem}"
              "h1{font-size:1.25rem;margin:0 0 1rem}"
              "label{display:block;margin:1rem 0 .25rem;font-weight:600}"
              "input{width:100%%;box-sizing:border-box;padding:.6rem;border-radius:6px;"
              "border:1px solid #444;background:#1c1c1c;color:#eee;font:inherit}"
              ".h{margin:.25rem 0 0;color:#999;font-size:.85rem}"
              "button{margin-top:1.5rem;width:100%%;padding:.7rem;border:0;"
              "border-radius:6px;font:inherit;font-weight:600;cursor:pointer}"
              ".save{background:#2d7;color:#052}"
              ".danger{background:#711;color:#fdd}"
              "hr{border:0;border-top:1px solid #333;margin:2rem 0}"
              "</style></head><body><h1>Daily Mirror settings</h1>"
              "<form method=post action=\"/config\">");

    render_text_field(&sb, MIRROR_CFG_WIFI_SSID, "Wi-Fi network (SSID)",
                      mirror_config_get(MIRROR_CFG_WIFI_SSID),
                      "1-32 characters. 2.4 GHz networks only.");
    render_secret_field(&sb, MIRROR_CFG_WIFI_PASS, "Wi-Fi password", pass_set);
    render_text_field(&sb, MIRROR_CFG_SERVER_URL, "Server URL",
                      mirror_config_get(MIRROR_CFG_SERVER_URL),
                      "Starts with http:// or https://. Blank to disable uploads.");
    render_secret_field(&sb, MIRROR_CFG_UPLOAD_TOKEN, "Upload token", token_set);
    render_text_field(&sb, MIRROR_CFG_DEVICE_NAME, "Device name",
                      mirror_config_get(MIRROR_CFG_DEVICE_NAME),
                      "Shown in the app, e.g. \"Hall camera\".");

    sb_printf(&sb,
              "<button class=save type=submit>Save and restart</button></form>"
              "<hr><form method=post action=\"/config/reset\" "
              "onsubmit=\"return confirm('Erase Wi-Fi, server URL and token from "
              "this camera?')\">"
              "<button class=danger type=submit>Forget everything</button></form>"
              "<p class=h>Saving restarts the camera so it rejoins Wi-Fi.</p>"
              "</body></html>");

    httpd_resp_set_type(req, "text/html");
    return httpd_resp_send(req, page, sb.len);
}

/* Read a value out of the body; returns an error message or NULL. */
static const char *read_field(const char *body, size_t len, const char *key,
                              char *out, bool *present)
{
    int rc = mirror_form_get(body, len, key, out, MIRROR_CONFIG_VALUE_MAX);
    *present = (rc == MIRROR_FORM_OK);
    switch (rc) {
    case MIRROR_FORM_OK:
    case MIRROR_FORM_ERR_NOT_FOUND:
        return NULL;
    case MIRROR_FORM_ERR_TOO_LONG:
        return "A value is too long for this device.";
    case MIRROR_FORM_ERR_BAD_ESCAPE:
        return "The form data was malformed (bad percent-escape).";
    default:
        return "The form data could not be read.";
    }
}

static esp_err_t post_config_handler(httpd_req_t *req)
{
    if (req->content_len > MIRROR_FORM_BODY_MAX) {
        return send_error(req, "That form was too large.");
    }

    /* Static so a 2 KB body never lands on the httpd task stack. */
    static char body[MIRROR_FORM_BODY_MAX + 1];
    size_t total = 0;
    while (total < (size_t)req->content_len) {
        int r = httpd_req_recv(req, body + total, (size_t)req->content_len - total);
        if (r == HTTPD_SOCK_ERR_TIMEOUT) continue;
        if (r <= 0) return ESP_FAIL;
        total += (size_t)r;
    }
    body[total] = '\0';

    char ssid[MIRROR_CONFIG_VALUE_MAX];
    char pass[MIRROR_CONFIG_VALUE_MAX];
    char url[MIRROR_CONFIG_VALUE_MAX];
    char token[MIRROR_CONFIG_VALUE_MAX];
    char name[MIRROR_CONFIG_VALUE_MAX];
    bool have_ssid, have_pass, have_url, have_token, have_name;
    const char *err;

    if ((err = read_field(body, total, MIRROR_CFG_WIFI_SSID,    ssid,  &have_ssid))  != NULL ||
        (err = read_field(body, total, MIRROR_CFG_WIFI_PASS,    pass,  &have_pass))  != NULL ||
        (err = read_field(body, total, MIRROR_CFG_SERVER_URL,   url,   &have_url))   != NULL ||
        (err = read_field(body, total, MIRROR_CFG_UPLOAD_TOKEN, token, &have_token)) != NULL ||
        (err = read_field(body, total, MIRROR_CFG_DEVICE_NAME,  name,  &have_name))  != NULL) {
        return send_error(req, err);
    }

    /* A blank secret means "leave unchanged", so it is not a change at all. */
    bool set_pass  = have_pass  && pass[0]  != '\0';
    bool set_token = have_token && token[0] != '\0';

    /* Validate everything before storing anything: no partial writes. */
    if (have_ssid && (err = mirror_validate_ssid(ssid)) != NULL) return send_error(req, err);
    if (set_pass  && (err = mirror_validate_pass(pass)) != NULL) return send_error(req, err);
    if (have_url  && (err = mirror_validate_url(url))   != NULL) return send_error(req, err);
    if (have_name && (err = mirror_validate_device_name(name)) != NULL) {
        return send_error(req, err);
    }
    /* Clearing the SSID while a password is stored would leave a useless secret
     * behind; a blank SSID field is simply rejected above by the validator. */

    esp_err_t e = ESP_OK;
    if (have_ssid)  e = mirror_config_set(MIRROR_CFG_WIFI_SSID, ssid);
    if (e == ESP_OK && set_pass)  e = mirror_config_set(MIRROR_CFG_WIFI_PASS, pass);
    if (e == ESP_OK && have_url)  e = mirror_config_set(MIRROR_CFG_SERVER_URL, url);
    if (e == ESP_OK && set_token) e = mirror_config_set(MIRROR_CFG_UPLOAD_TOKEN, token);
    if (e == ESP_OK && have_name) e = mirror_config_set(MIRROR_CFG_DEVICE_NAME, name);
    if (e != ESP_OK) {
        ESP_LOGE(TAG, "store failed: %s", esp_err_to_name(e));
        return send_error(req, "The settings could not be stored on this device.");
    }

    ESP_LOGI(TAG, "settings updated via /config"); /* never log the values */
    return send_rebooting(req, "Saved");
}

static esp_err_t post_reset_handler(httpd_req_t *req)
{
    /* Drain whatever the browser sent so the connection stays in sync. */
    char scratch[64];
    size_t left = (size_t)req->content_len;
    while (left > 0) {
        size_t want = left < sizeof(scratch) ? left : sizeof(scratch);
        int r = httpd_req_recv(req, scratch, want);
        if (r == HTTPD_SOCK_ERR_TIMEOUT) continue;
        if (r <= 0) break;
        left -= (size_t)r;
    }

    esp_err_t e = mirror_config_erase_all();
    if (e != ESP_OK) return send_error(req, "The settings could not be erased.");
    return send_rebooting(req, "Erased");
}

/* Handler descriptors must outlive the call: httpd copies them, but keeping
 * them static costs nothing and matches the documented contract. */
static const httpd_uri_t s_uri_get_config = {
    .uri = "/config", .method = HTTP_GET, .handler = get_config_handler, .user_ctx = NULL,
};
static const httpd_uri_t s_uri_post_config = {
    .uri = "/config", .method = HTTP_POST, .handler = post_config_handler, .user_ctx = NULL,
};
static const httpd_uri_t s_uri_post_reset = {
    .uri = "/config/reset", .method = HTTP_POST, .handler = post_reset_handler, .user_ctx = NULL,
};

esp_err_t mirror_config_register_http(httpd_handle_t server)
{
    if (server == NULL) return ESP_ERR_INVALID_ARG;

    esp_err_t err = httpd_register_uri_handler(server, &s_uri_get_config);
    if (err == ESP_OK) err = httpd_register_uri_handler(server, &s_uri_post_config);
    if (err == ESP_OK) err = httpd_register_uri_handler(server, &s_uri_post_reset);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "register failed: %s (raise max_uri_handlers?)", esp_err_to_name(err));
    }
    return err;
}
