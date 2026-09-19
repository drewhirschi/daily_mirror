cmake_minimum_required(VERSION 3.16)

# ---------------------------------------------------------------------------
# Board selection.
#
#   idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 build
#   idf.py -B build_s3 -DMIRROR_BOARD=s3_ov5640 build
#
# One variable does everything, because the three things it has to set are not
# independent and getting them out of step produces bad errors: the IDF target,
# the sdkconfig defaults, and the Kconfig board choice (which lives in
# boards/<name>/sdkconfig.defaults and picks the adapter source). It is cached,
# so it only has to be passed the first time a build directory is configured.
#
# sdkconfig is redirected into the build directory. It is per-board - one names
# esp32p4 and pulls esp_hosted, the other names esp32s3 and pulls esp32-camera -
# and in its stock location building one board would rewrite the other's, so
# the next build of that one would start with a full reconfigure. Under the
# build directory the two simply coexist, and neither is ever a candidate for
# committing.
#
# dependencies.lock cannot be moved the same way (the component manager writes
# it beside the manifest regardless), so it stays in the project directory, is
# rewritten on every board switch, and is git-ignored. The consequence is that
# switching boards re-solves dependencies; the solve is seconds and the
# download is cached, so this is a shrug rather than a problem.
# ---------------------------------------------------------------------------
set(MIRROR_BOARDS p4_imx519 s3_ov5640)
set(MIRROR_BOARD "" CACHE STRING "Target board: ${MIRROR_BOARDS}")
set_property(CACHE MIRROR_BOARD PROPERTY STRINGS ${MIRROR_BOARDS})

if(NOT MIRROR_BOARD)
    message(FATAL_ERROR
        "No board selected. Pass -DMIRROR_BOARD=<board>, one of: ${MIRROR_BOARDS}\n"
        "  idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 build\n"
        "  idf.py -B build_s3 -DMIRROR_BOARD=s3_ov5640 build\n"
        "or use the justfile recipes: just fw-build-p4 / just fw-build-s3")
endif()
if(NOT MIRROR_BOARD IN_LIST MIRROR_BOARDS)
    message(FATAL_ERROR "Unknown MIRROR_BOARD \"${MIRROR_BOARD}\". Pick one of: ${MIRROR_BOARDS}")
endif()

set(mirror_board_dir "${CMAKE_CURRENT_LIST_DIR}/boards/${MIRROR_BOARD}")
if(MIRROR_BOARD STREQUAL "p4_imx519")
    set(IDF_TARGET "esp32p4")
elseif(MIRROR_BOARD STREQUAL "s3_ov5640")
    set(IDF_TARGET "esp32s3")
endif()

set(SDKCONFIG_DEFAULTS
    "${CMAKE_CURRENT_LIST_DIR}/sdkconfig.defaults"
    "${mirror_board_dir}/sdkconfig.defaults")

# Bench credentials, without putting them in the repository.
#
# mirror_config's build-time fallbacks are Kconfig strings
# (MIRROR_DEFAULT_WIFI_SSID and friends), and a Kconfig string cannot be set
# with -D on the command line. So: point MIRROR_EXTRA_SDKCONFIG at a file
# OUTSIDE this tree and it is applied last, after the board's defaults.
#
#   idf.py -B build_p4 -DMIRROR_BOARD=p4_imx519 \
#          -DMIRROR_EXTRA_SDKCONFIG=~/esp/bench.sdkconfig build
#
# Nothing in the repository ever needs an SSID, a password or a token: a device
# built without this file comes up on the setup network and is configured
# through /config. This exists so a bench unit can be flashed and land straight
# on the house network.
if(MIRROR_EXTRA_SDKCONFIG)
    get_filename_component(mirror_extra_sdkconfig "${MIRROR_EXTRA_SDKCONFIG}" ABSOLUTE)
    if(NOT EXISTS "${mirror_extra_sdkconfig}")
        message(FATAL_ERROR "MIRROR_EXTRA_SDKCONFIG does not exist: ${mirror_extra_sdkconfig}")
    endif()
    if(mirror_extra_sdkconfig MATCHES "^${CMAKE_CURRENT_LIST_DIR}/")
        message(FATAL_ERROR
            "MIRROR_EXTRA_SDKCONFIG must live outside the repository - it is for "
            "credentials:\n  ${mirror_extra_sdkconfig}")
    endif()
    list(APPEND SDKCONFIG_DEFAULTS "${mirror_extra_sdkconfig}")
    message(STATUS "Applying extra sdkconfig defaults from ${mirror_extra_sdkconfig}")
endif()

set(SDKCONFIG "${CMAKE_BINARY_DIR}/sdkconfig")

include($ENV{IDF_PATH}/tools/cmake/project.cmake)

if(MIRROR_BOARD STREQUAL "p4_imx519")
    # -----------------------------------------------------------------------
    # Hand the IMX519's ISP tuning config to esp_ipa. This has to sit between
    # project.cmake and project(): esp_ipa reads the property while its own
    # CMakeLists is processed, which is too late for a component to set it.
    #
    # Get it wrong and nothing complains. esp_ipa's existence check is spelled
    # message(FETAL_ERROR ...), which is not a real CMake mode, so a missing
    # file prints a line and the build carries on to produce a stream with no
    # auto-exposure and no white balance. main/ re-checks the resolved path
    # below and fails the build instead.
    # -----------------------------------------------------------------------
    set(imx519_ipa_json
        "${CMAKE_CURRENT_LIST_DIR}/components/esp_cam_sensor_imx/sensors/imx519/cfg/imx519_default.json")
    if(NOT EXISTS "${imx519_ipa_json}")
        message(FATAL_ERROR
            "IMX519 ISP tuning config not found at:\n  ${imx519_ipa_json}\n"
            "esp_ipa would silently skip it and the camera would run with no "
            "auto-exposure and no white balance.")
    endif()
    idf_build_set_property(ESP_IPA_JSON_CONFIG_FILE_PATH "${imx519_ipa_json}" APPEND)

    # -----------------------------------------------------------------------
    # ABI fix: make esp_ipa_stats_t match the prebuilt esp_ipa binary.
    #
    # esp_ipa ships as a closed .a compiled with the AWB sub-window grid
    # present, i.e. with ISP_AWB_WINDOW_X_NUM / _Y_NUM defined as 5. Those
    # macros only exist in recent ESP-IDF. Below that they are undefined, so
    # esp_ipa_types.h collapses esp_ipa_stats_t::awb_subwin to a [0][0] array -
    # 400 bytes shorter than the library expects, which puts af_stats at offset
    # 196 in a 224-byte struct while the library reads it from 596. Autofocus
    # then scores unrelated heap memory and always settles on its first scan
    # point, and auto-exposure reads a luma of 0.03 off a blown-out frame.
    #
    # This has to be global - esp_video writes the struct, the generated IPA
    # config declares it, and the library reads it, so all three must agree.
    # From v5.4.4 / v5.5.3 ESP-IDF defines the macros itself and this steps
    # aside. The bench is on v5.5.5, so this branch is not taken there; it is
    # kept for anyone who builds on an older toolchain, where the symptom
    # ("autofocus does nothing") points nowhere near the cause.
    # -----------------------------------------------------------------------
    if((IDF_VERSION_MAJOR EQUAL 5 AND IDF_VERSION_MINOR EQUAL 4 AND IDF_VERSION_PATCH LESS 4) OR
       (IDF_VERSION_MAJOR EQUAL 5 AND IDF_VERSION_MINOR EQUAL 5 AND IDF_VERSION_PATCH LESS 3))
        message(STATUS "esp_ipa ABI guard active for ESP-IDF ${IDF_VERSION_MAJOR}.${IDF_VERSION_MINOR}.${IDF_VERSION_PATCH}")
        idf_build_set_property(COMPILE_DEFINITIONS "ISP_AWB_WINDOW_X_NUM=5" APPEND)
        idf_build_set_property(COMPILE_DEFINITIONS "ISP_AWB_WINDOW_Y_NUM=5" APPEND)
    endif()
endif()

project(daily_mirror_camera)
