idf_component_register(
    SRCS "mirror_main.c"
         "mirror_net.c"
         "mirror_http.c"
         "mirror_press.c"
         "mirror_ring.c"
         "mirror_upload.c"
    INCLUDE_DIRS "."
    REQUIRES mirror_board mirror_config mirror_mdns
    PRIV_REQUIRES esp_wifi esp_netif esp_event nvs_flash esp_http_server
                  esp_http_client esp-tls json esp_timer esp_driver_gpio
)
