/* Headers bindgen turns into the `esp_idf_sys` FFI surface for this crate.
 *
 * Every adapter module in src/ reaches its C API through exactly one group
 * below. Keeping the list short keeps the generated bindings small and the
 * build fast; add a header only when an adapter actually needs it. */

/* Hosted Wi-Fi: the same esp_wifi_* API as a radio-bearing chip, forwarded
 * over SDIO to the companion C6 by esp_wifi_remote. net.rs. */
#include "esp_wifi.h"
#include "esp_wifi_remote.h"
#include "esp_hosted.h"
#include "esp_netif.h"
#include "esp_event.h"

/* SoftAP provisioning and the custom endpoint carrying server_url and
 * claim_token. net.rs. */
#include "network_provisioning/manager.h"
#include "network_provisioning/scheme_softap.h"

/* Camera: V4L2-style capture, the ISP pipeline, and the JPEG encoder.
 * camera.rs. */
#include "esp_video_init.h"
#include "esp_video_device.h"
#include "linux/videodev2.h"

/* The IMX519 sensor and its DW9714 focus motor. camera.rs. */
#include "esp_cam_sensor.h"
#include "imx519.h"

/* Storage: NVS for credentials and the device token, FAT on SD for the photo
 * queue. store.rs. */
#include "nvs_flash.h"
#include "nvs.h"
#include "esp_vfs_fat.h"
#include "sdmmc_cmd.h"
#include "driver/sdmmc_host.h"

/* Ring: WS2812 over the RMT peripheral. ring.rs. */
#include "driver/rmt_tx.h"

/* Button, clock, reboot. button.rs, clock.rs, main.rs. */
#include "driver/gpio.h"
#include "esp_timer.h"
#include "esp_system.h"
