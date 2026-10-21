# esp_hosted exists only in the P4 build (mirror_board pulls it in for that
# target); mirror_link.c compiles to a stub everywhere else. Like
# mirror_board's REQUIRES, this follows IDF_TARGET because Kconfig is not
# loaded yet when requirements are expanded.
idf_build_get_property(mirror_target IDF_TARGET)
set(mirror_link_reqs)
if(mirror_target STREQUAL "esp32p4")
    list(APPEND mirror_link_reqs esp_hosted)
endif()

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
         "mirror_serial.c"
         "mirror_link.c"
    INCLUDE_DIRS "."
    REQUIRES mirror_board mirror_config mirror_mdns
    PRIV_REQUIRES esp_wifi esp_netif esp_event nvs_flash esp_http_server
                  esp_http_client esp-tls json esp_timer esp_driver_gpio esp_driver_uart
                  # Pairing: the provisioning manager, its BLE scheme, and
                  # protocomm for the security 2 salt/verifier helper.
                  # wifi_prov_remote supplies the manager's implementation on
                  # the P4, where IDF's own component is headers only.
                  wifi_provisioning wifi_prov_remote protocomm
                  # The upload spool: LittleFS on its own data partition, and
                  # esp_app_desc for the firmware version in capture metadata.
                  joltwallet__littlefs app_update
                  # The co-processor link watchdog (P4 only).
                  ${mirror_link_reqs}
)
