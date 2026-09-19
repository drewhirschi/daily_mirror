idf_component_register(
    SRCS "mirror_config.c" "mirror_form.c"
    INCLUDE_DIRS "include"
    # esp_http_server is in the public header (httpd_handle_t), so it is a
    # public requirement; the storage and restart plumbing is private.
    REQUIRES esp_http_server
    PRIV_REQUIRES nvs_flash esp_system freertos
)
