idf_component_register(
    SRCS "mirror_mdns.c"
    INCLUDE_DIRS "include"
    # The public header only needs esp_err/stdbool, so everything is private.
    # espressif__mdns comes from idf_component.yml and is pulled in implicitly,
    # but naming it keeps the dependency obvious.
    PRIV_REQUIRES esp_wifi esp_netif esp_hw_support espressif__mdns
)
