# SPDX-License-Identifier: Apache-2.0
#
# esp_cam_sensor_imx — trimmed to the parts Daily Mirror uses: the Sony IMX519
# sensor and the AK7375 focus motor. Upstream (mushBrainDave/esp32-p4-imx-camera)
# also carries IMX219, IMX708 and the DW9807 VCM; see README.md.
#
# Standalone add-on: it plugs into esp_cam_sensor's auto-detect array via the
# ".esp_cam_sensor_detect_fn" linker section (esp_cam_sensor's linker.lf gathers
# that section from *all* archives).

set(srcs)
set(include_dirs)
set(priv_include_dirs)

if(CONFIG_CAMERA_IMX519)
    list(APPEND srcs "sensors/imx519/imx519.c")
    list(APPEND include_dirs "sensors/imx519/include")
    list(APPEND priv_include_dirs "sensors/imx519/private_include")
endif()

# The autofocus actuator is a separate chip from the sensor, on its own I2C
# address (0x0c), with its own detect array (".esp_cam_motor_detect_fn").
if(CONFIG_CAM_MOTOR_AK7375)
    list(APPEND srcs "motors/ak7375/ak7375.c")
    list(APPEND include_dirs "motors/ak7375/include")
endif()

# ISP tuning (IPA) configs live in sensors/<name>/cfg/<name>_default.json, but
# they CANNOT be registered from here: esp_ipa reads ESP_IPA_JSON_CONFIG_FILE_PATH
# while its own CMakeLists is processed, which has already happened by the time a
# component's CMakeLists runs. The project CMakeLists registers it, between
# include(project.cmake) and project(). Without that registration
# esp_ipa_pipeline_get_config("IMX519") returns NULL, esp_video logs "failed to
# get configuration to initialize ISP controller", and the stream comes back
# with no auto-exposure and no white balance.

# On a non-P4 board none of the above is selected and esp_cam_sensor is not
# even resolved, so register an empty component rather than a broken one: it
# costs nothing in the image and keeps the S3 build from having to know this
# directory exists.
idf_build_get_property(imx_target IDF_TARGET)
if(NOT imx_target STREQUAL "esp32p4")
    idf_component_register()
    return()
endif()

idf_component_register(
    SRCS ${srcs}
    INCLUDE_DIRS ${include_dirs}
    PRIV_INCLUDE_DIRS ${priv_include_dirs}
    REQUIRES esp_cam_sensor
    PRIV_REQUIRES esp_driver_gpio esp_sccb_intf esp_timer
)

# Force the linker to keep each detect object so its entry lands in the
# auto-detect array. Without this the linker drops the object, since nothing in
# the app references it directly.
if(CONFIG_CAMERA_IMX519)
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-u imx519_detect")
endif()

if(CONFIG_CAM_MOTOR_AK7375)
    target_link_libraries(${COMPONENT_LIB} INTERFACE "-u ak7375_detect")
endif()
