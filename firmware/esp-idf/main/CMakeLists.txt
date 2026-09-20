idf_component_register(
    SRCS "mirror_main.c"
         "mirror_net.c"
         "mirror_http.c"
         "mirror_pair.c"
         "mirror_press.c"
         "mirror_ring.c"
         "mirror_upload.c"
         "mirror_spool.c"
         "mirror_log.c"
    INCLUDE_DIRS "."
    REQUIRES mirror_board mirror_config mirror_mdns
    PRIV_REQUIRES esp_wifi esp_netif esp_event nvs_flash esp_http_server
                  esp_http_client esp-tls json esp_timer esp_driver_gpio
                  # Pairing: the provisioning manager, its BLE scheme, and
                  # protocomm for the security 2 salt/verifier helper.
                  wifi_provisioning protocomm
                  # The upload spool: LittleFS on its own data partition, and
                  # esp_app_desc for the firmware version in capture metadata.
                  joltwallet__littlefs app_update
)
