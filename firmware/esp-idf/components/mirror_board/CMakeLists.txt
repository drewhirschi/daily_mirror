#
# One adapter source per build.
#
# Note the two different selectors below, which is deliberate and not an
# oversight. SRCS are read on the normal pass, where CONFIG_* is available, so
# the source file follows the Kconfig board choice. REQUIRES are read on the
# early-expansion pass, which runs *before* Kconfig, so they cannot; they follow
# IDF_TARGET, which idf_build_get_property does know that early. That is fine
# while board and target are one-to-one - and if a second board ever shares a
# target, only the dependency list needs revisiting, since a superset of
# REQUIRES costs nothing but a slightly wider build graph.

idf_build_get_property(mirror_target IDF_TARGET)

set(srcs)
set(reqs esp_driver_gpio esp_timer)

if(CONFIG_MIRROR_BOARD_P4_IMX519)
    list(APPEND srcs "src/board_p4_imx519.c")
elseif(CONFIG_MIRROR_BOARD_S3_OV5640)
    list(APPEND srcs "src/board_s3_ov5640.c")
endif()

if(mirror_target STREQUAL "esp32p4")
    list(APPEND reqs esp_video esp_cam_sensor_imx esp_driver_jpeg esp_driver_ledc)
elseif(mirror_target STREQUAL "esp32s3")
    list(APPEND reqs esp32-camera led_strip)
endif()

idf_component_register(
    SRCS ${srcs}
    INCLUDE_DIRS "include"
    PRIV_INCLUDE_DIRS "src"
    REQUIRES ${reqs}
    PRIV_REQUIRES esp_wifi esp_netif
)
