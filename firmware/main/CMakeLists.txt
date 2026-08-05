idf_component_register(
    SRCS "elduro_bridge.c"
    INCLUDE_DIRS "."
    REQUIRES nvs_flash bt esp_psram spi_flash
)
