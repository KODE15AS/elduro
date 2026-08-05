// elduro field bridge - smoke test firmware.
//
// Proves the toolchain and hardware end to end without needing the microSD
// card: reports chip and PSRAM, brings up the NimBLE host, and runs a passive
// BLE scan that logs every advertiser it sees. A Polar H10 in range is flagged
// so we can confirm the radio reaches the sensor before wiring up PMD streaming.
// A 5 s heartbeat also reports free heap and free PSRAM so any log capture
// window shows the memory state.

#include <string.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "esp_chip_info.h"
#include "esp_flash.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_psram.h"
#include "nvs_flash.h"

#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "host/util/util.h"
#include "host/ble_gap.h"

static const char *TAG = "elduro";

static void log_hardware(void)
{
    esp_chip_info_t chip;
    esp_chip_info(&chip);

    uint32_t flash_size = 0;
    esp_flash_get_size(NULL, &flash_size);

    ESP_LOGI(TAG, "chip: ESP32-S3 rev v%d.%d, %d core(s)",
             chip.revision / 100, chip.revision % 100, chip.cores);
    ESP_LOGI(TAG, "flash: %lu MB", (unsigned long)(flash_size / (1024 * 1024)));

#if CONFIG_SPIRAM
    size_t psram = esp_psram_get_size();
    if (psram > 0) {
        ESP_LOGI(TAG, "PSRAM: %u MB detected (octal config OK)",
                 (unsigned)(psram / (1024 * 1024)));
    } else {
        ESP_LOGW(TAG, "PSRAM: enabled in config but 0 bytes detected - wrong mode?");
    }
#else
    ESP_LOGW(TAG, "PSRAM: disabled in config");
#endif
}

static int gap_event(struct ble_gap_event *event, void *arg)
{
    if (event->type != BLE_GAP_EVENT_DISC) {
        return 0;
    }

    struct ble_hs_adv_fields fields;
    if (ble_hs_adv_parse_fields(&fields, event->disc.data,
                                event->disc.length_data) != 0) {
        return 0;
    }

    char name[32] = {0};
    if (fields.name != NULL && fields.name_len > 0) {
        int n = fields.name_len < (int)sizeof(name) - 1
                    ? fields.name_len
                    : (int)sizeof(name) - 1;
        memcpy(name, fields.name, n);
    }

    const uint8_t *a = event->disc.addr.val; // little-endian
    bool is_polar = strstr(name, "Polar") != NULL;

    ESP_LOGI(TAG, "%s %02x:%02x:%02x:%02x:%02x:%02x  rssi=%3d  name='%s'",
             is_polar ? ">> POLAR H10 <<" : "adv",
             a[5], a[4], a[3], a[2], a[1], a[0], event->disc.rssi, name);
    return 0;
}

static void start_scan(void)
{
    uint8_t own_addr_type;
    if (ble_hs_id_infer_auto(0, &own_addr_type) != 0) {
        ESP_LOGE(TAG, "no usable BLE address");
        return;
    }

    struct ble_gap_disc_params disc = {
        .passive = 1,
        .filter_duplicates = 0,
    };
    int rc = ble_gap_disc(own_addr_type, BLE_HS_FOREVER, &disc, gap_event, NULL);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gap_disc failed: %d", rc);
        return;
    }
    ESP_LOGI(TAG, "scanning for BLE advertisers (put the Polar H10 on to see it)");
}

static void on_sync(void)
{
    ble_hs_util_ensure_addr(0);
    start_scan();
}

static void on_reset(int reason)
{
    ESP_LOGW(TAG, "BLE host reset, reason=%d", reason);
}

static void host_task(void *param)
{
    nimble_port_run();
    nimble_port_freertos_deinit();
}

void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ESP_ERROR_CHECK(nvs_flash_init());
    }

    ESP_LOGI(TAG, "elduro field bridge - smoke test boot");
    log_hardware();

    ESP_ERROR_CHECK(nimble_port_init());
    ble_hs_cfg.sync_cb = on_sync;
    ble_hs_cfg.reset_cb = on_reset;
    nimble_port_freertos_init(host_task);

    while (1) {
        vTaskDelay(pdMS_TO_TICKS(5000));
        ESP_LOGI(TAG, "heartbeat: free heap=%u B, free PSRAM=%u B",
                 (unsigned)esp_get_free_heap_size(),
                 (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    }
}
