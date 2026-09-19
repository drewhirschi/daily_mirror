/*
 * Daily Mirror camera firmware. One app, two boards.
 *
 * Everything hardware-specific is behind mirror_board.h; everything here is
 * the same on the ESP32-P4 + IMX519 rig and the ESP32-S3 + OV5640 rig. Pick
 * the board with -DMIRROR_BOARD=p4_imx519|s3_ov5640 - see firmware/README.md.
 *
 * Start-up order is deliberate:
 *   settings -> LED -> camera -> network -> server -> mDNS -> button
 * The LED comes up early so a failure has somewhere to show itself; the camera
 * comes up before the network so the ISP's AE/AWB/AF convergence, which takes
 * a few seconds of frames, overlaps the Wi-Fi association instead of following
 * it; and the button is last so a press cannot arrive before there is anywhere
 * to put the photo.
 *
 * Nothing here calls esp_restart() on a failure. A camera that did not answer
 * still has to serve /config, because /config is how someone fixes the thing
 * that is wrong.
 */
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_log.h"

#include "mirror_board.h"
#include "mirror_config.h"
#include "mirror_mdns.h"
#include "mirror_net.h"
#include "mirror_http.h"
#include "mirror_press.h"
#include "mirror_ring.h"

static const char *TAG = "mirror";

void app_main(void)
{
    ESP_LOGI(TAG, "Daily Mirror %s on %s", CONFIG_MIRROR_FW_VERSION, board_name());

#if CONFIG_MIRROR_DEBUG_ISP_STATS
    /* The ISP's 3A chatter: every exposure decision, white-balance gain and
     * focus score. Invaluable on the bench, unreadable otherwise, so it is
     * behind a Kconfig flag rather than deleted. */
    esp_log_level_set("esp_ipa*", ESP_LOG_DEBUG);
    esp_log_level_set("esp_video*", ESP_LOG_DEBUG);
    ESP_LOGW(TAG, "ISP stats logging is on (MIRROR_DEBUG_ISP_STATS)");
#endif

    /* Settings first: the network needs them, and this is also what
     * initialises NVS for everything else. */
    ESP_ERROR_CHECK(mirror_config_init());

    if (board_led_init() == ESP_OK) {
        mirror_ring_start();
    }
    mirror_ring_set(RING_BLUE_SLOW_PULSE);   /* joining */

    bool camera_ok = board_camera_init() == ESP_OK;
    if (!camera_ok) {
        /* Carry on: the admin server still has to come up so /config is
         * reachable, and on this rig a camera that failed to detect is usually
         * the ribbon rather than the firmware. */
        ESP_LOGE(TAG, "camera init failed - serving without a camera");
    }

    if (mirror_net_start() != ESP_OK) {
        ESP_LOGE(TAG, "no network at all - nothing further can be served");
        mirror_ring_set(RING_SOLID_RED);
        return;
    }

    httpd_handle_t server = NULL;
    if (mirror_http_start(&server) != ESP_OK) {
        mirror_ring_set(RING_SOLID_RED);
        return;
    }

    mirror_mdns_start(&(mirror_mdns_info_t){
        .board = board_id(),
        .fw_version = CONFIG_MIRROR_FW_VERSION,
        .claimed = false,
        .http_port = 80,
    });

    mirror_press_start();

    if (mirror_net_ap_active()) {
        /* Unprovisioned: works offline, not yet on a network anyone can find
         * it on. The breathe is the ring.rs pattern for exactly that. */
        mirror_ring_set(RING_DIM_WHITE_BREATHE);
        ESP_LOGW(TAG, "setup network \"%s\" - join it and open http://10.10.0.1/config",
                 mirror_net_ap_ssid());
    } else if (camera_ok) {
        mirror_ring_set(RING_SOLID_WHITE);
    } else {
        mirror_ring_set(RING_SOLID_RED);
    }

    ESP_LOGI(TAG, "admin at http://%s/  (http://%s.local/)",
             mirror_net_ip(), mirror_mdns_hostname() ? mirror_mdns_hostname() : "?");
    /* The marker tools/capture.py waits for; everything past this point
     * happens in the server and the button task. */
    printf("\n==== ready - serving at http://%s/ ====\n", mirror_net_ip());
}
