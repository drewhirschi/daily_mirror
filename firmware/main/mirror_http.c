#include <string.h>
#include <inttypes.h>

#include "mirror_http.h"
#include "mirror_board.h"
#include "mirror_net.h"
#include "mirror_press.h"
#include "mirror_upload.h"
#include "mirror_config.h"
#include "mirror_mdns.h"

#include "esp_log.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"

static const char *TAG = "mirror_http";

static esp_err_t index_handler(httpd_req_t *req)
{
    char page[1600];
    char device_id[13];
    mirror_device_id(device_id);

    snprintf(page, sizeof(page),
        "<!doctype html><meta name=viewport content=\"width=device-width,initial-scale=1\">"
        "<title>%s</title>"
        "<style>body{margin:0;background:#111;color:#eee;font:15px system-ui;text-align:center}"
        "img{max-width:100%%;height:auto}button,a{font:inherit;padding:8px 14px;margin:4px;"
        "color:#6cf;background:#222;border:1px solid #444;border-radius:6px;display:inline-block;"
        "text-decoration:none}</style>"
        "<h3 style=margin:10px>%s</h3>"
        "<p><img id=v src=\"/snapshot.jpg\">"
        "<p><button onclick=\"v.src='/snapshot.jpg?'+Date.now()\">snapshot</button>"
        "<button onclick=\"fetch('/press',{method:'POST'}).then(r=>r.text())"
        ".then(t=>{alert(t);v.src='/last.jpg?'+Date.now()})\">press the button</button>"
        "<a href=/last.jpg>last photo</a><a href=/config>settings</a><a href=/stats>stats</a>"
        "<p style=color:#888>%s &middot; id %s &middot; %s &middot; upload: %s",
        board_name(), board_name(), board_camera_status(), device_id,
        mirror_net_ap_active() ? "setup network" : mirror_net_ip(),
        mirror_upload_last_result());

    httpd_resp_set_type(req, "text/html");
    return httpd_resp_send(req, page, HTTPD_RESP_USE_STRLEN);
}

static esp_err_t snapshot_handler(httpd_req_t *req)
{
    uint8_t *jpeg = NULL;
    size_t len = 0;
    esp_err_t err = board_camera_capture(true, &jpeg, &len);
    if (err != ESP_OK || !jpeg) {
        board_camera_release(jpeg);
        httpd_resp_send_err(req, HTTPD_500_INTERNAL_SERVER_ERROR, "capture failed");
        return ESP_FAIL;
    }
    httpd_resp_set_type(req, "image/jpeg");
    /* No caching: every request should get the live scene, not the last one. */
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    err = httpd_resp_send(req, (const char *)jpeg, len);
    board_camera_release(jpeg);
    return err;
}

static esp_err_t last_handler(httpd_req_t *req)
{
    const uint8_t *jpeg = NULL;
    size_t len = 0;
    if (!mirror_press_last_acquire(&jpeg, &len)) {
        httpd_resp_set_status(req, "404 Not Found");
        return httpd_resp_sendstr(req, "no press photo yet");
    }
    httpd_resp_set_type(req, "image/jpeg");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    esp_err_t err = httpd_resp_send(req, (const char *)jpeg, len);
    mirror_press_last_release();
    return err;
}

static esp_err_t press_handler(httpd_req_t *req)
{
    if (mirror_press_busy()) {
        return httpd_resp_sendstr(req, "busy");
    }
    uint32_t before = mirror_press_count();
    mirror_press_run();
    return httpd_resp_sendstr(req, mirror_press_count() > before ? "captured" : "failed");
}

static esp_err_t upload_handler(httpd_req_t *req)
{
    if (!mirror_upload_configured()) {
        httpd_resp_set_status(req, "409 Conflict");
        return httpd_resp_sendstr(req, "set server_url and upload_token in /config first");
    }
    uint8_t *jpeg = NULL;
    size_t len = 0;
    if (board_camera_capture(true, &jpeg, &len) != ESP_OK || !jpeg) {
        board_camera_release(jpeg);
        httpd_resp_send_err(req, HTTPD_500_INTERNAL_SERVER_ERROR, "capture failed");
        return ESP_FAIL;
    }
    mirror_upload_jpeg(jpeg, len);
    board_camera_release(jpeg);
    return httpd_resp_sendstr(req, mirror_upload_last_result());
}

static esp_err_t stats_handler(httpd_req_t *req)
{
    char device_id[13];
    mirror_device_id(device_id);
    uint32_t w = 0, h = 0;
    board_camera_last_size(&w, &h);

    char body[512];
    int n = snprintf(body, sizeof(body),
        "board=%s\n" "board_id=%s\n" "device_id=%s\n" "fw=%s\n"
        "camera=%s\n" "frame=%" PRIu32 "x%" PRIu32 "\n"
        "ip=%s\n" "sta_connected=%d\n" "ap_active=%d\n" "ap_ssid=%s\n"
        "hostname=%s\n" "presses=%" PRIu32 "\n" "last_upload=%s\n"
        "uptime_s=%" PRIu64 "\n" "heap_internal=%u\n" "heap_psram=%u\n",
        board_name(), board_id(), device_id, CONFIG_MIRROR_FW_VERSION,
        board_camera_status(), w, h,
        mirror_net_ip(), mirror_net_sta_connected() ? 1 : 0,
        mirror_net_ap_active() ? 1 : 0, mirror_net_ap_ssid(),
        mirror_mdns_hostname() ? mirror_mdns_hostname() : "",
        mirror_press_count(), mirror_upload_last_result(),
        (uint64_t)(esp_timer_get_time() / 1000000),
        (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL),
        (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));

    httpd_resp_set_type(req, "text/plain");
    return httpd_resp_send(req, body, n);
}

esp_err_t mirror_http_start(httpd_handle_t *out)
{
    httpd_config_t cfg = HTTPD_DEFAULT_CONFIG();
    /* The capture path, the JPEG encoder and mbedTLS all run on handler tasks
     * and need far more than the 4 KB default. */
    cfg.stack_size = 12288;
    cfg.lru_purge_enable = true;
    /* A browser opens several sockets at once; without headroom the page and
     * the image it references request each other out. */
    cfg.max_open_sockets = 4;
    cfg.send_wait_timeout = 10;
    cfg.recv_wait_timeout = 10;
    /* Six here plus whatever mirror_config registers; the default 8 is not
     * enough, and the overflow shows up as a route that silently 404s. */
    cfg.max_uri_handlers = 16;

    httpd_handle_t server = NULL;
    esp_err_t err = httpd_start(&server, &cfg);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "httpd_start failed: %s", esp_err_to_name(err));
        return err;
    }

    static const httpd_uri_t uris[] = {
        { .uri = "/",             .method = HTTP_GET,  .handler = index_handler },
        { .uri = "/snapshot.jpg", .method = HTTP_GET,  .handler = snapshot_handler },
        { .uri = "/last.jpg",     .method = HTTP_GET,  .handler = last_handler },
        { .uri = "/press",        .method = HTTP_POST, .handler = press_handler },
        { .uri = "/upload",       .method = HTTP_POST, .handler = upload_handler },
        { .uri = "/stats",        .method = HTTP_GET,  .handler = stats_handler },
    };
    for (size_t i = 0; i < sizeof(uris) / sizeof(uris[0]); i++) {
        httpd_register_uri_handler(server, &uris[i]);
    }

    /* The settings form lives in its own component and registers itself here,
     * so /config is served by the same server on both the station and the
     * fallback access point. */
    err = mirror_config_register_http(server);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "mirror_config_register_http failed: %s", esp_err_to_name(err));
    }

    *out = server;
    return ESP_OK;
}
