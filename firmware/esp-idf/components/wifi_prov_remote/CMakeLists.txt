# Espressif's Wi-Fi provisioning manager, for a board whose Wi-Fi is remote.
#
# On the ESP32-P4 the radio is the companion ESP32-C6, reached through
# esp_hosted and esp_wifi_remote. ESP-IDF's own wifi_provisioning component
# sees neither CONFIG_ESP_WIFI_ENABLED nor CONFIG_ESP_HOST_WIFI_ENABLED there
# and registers its headers only, on the assumption that esp_wifi_remote ships
# the implementation - which no release of it does. The manager and the SoftAP
# scheme only call the public esp_wifi_* API, which esp_wifi_remote forwards to
# the C6, so the stock sources build and run unchanged. This compiles them from
# $IDF_PATH so they stay in step with the toolchain; mirror_pair uses them for
# SoftAP pairing.
#
# Everywhere else (the ESP32-S3, with local Wi-Fi) the IDF component is
# complete and this registers nothing.

if(NOT CMAKE_BUILD_EARLY_EXPANSION
   AND (CONFIG_ESP_WIFI_ENABLED OR CONFIG_ESP_HOST_WIFI_ENABLED OR NOT CONFIG_ESP_WIFI_REMOTE_ENABLED))
    idf_component_register()
    return()
endif()

set(prov "$ENV{IDF_PATH}/components/wifi_provisioning")

idf_component_register(
    SRCS "${prov}/src/wifi_config.c"
         "${prov}/src/wifi_scan.c"
         "${prov}/src/wifi_ctrl.c"
         "${prov}/src/manager.c"
         "${prov}/src/handlers.c"
         "${prov}/src/scheme_softap.c"
         "${prov}/proto-c/wifi_config.pb-c.c"
         "${prov}/proto-c/wifi_scan.pb-c.c"
         "${prov}/proto-c/wifi_ctrl.pb-c.c"
         "${prov}/proto-c/wifi_constants.pb-c.c"
    PRIV_INCLUDE_DIRS "${prov}/src" "${prov}/proto-c"
    REQUIRES wifi_provisioning lwip protocomm
    PRIV_REQUIRES protobuf-c json esp_timer esp_wifi esp_netif esp_event nvs_flash
)
