idf_component_register(
    SRCS "elduro_bridge.c"
    INCLUDE_DIRS "."
    REQUIRES nvs_flash esp_wifi esp_event esp_netif esp-tls esp_timer
             esp_driver_gpio esp_websocket_client bt esp_psram
)
