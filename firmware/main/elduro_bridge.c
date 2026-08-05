// elduro field bridge - step 2: stream raw ECG from the Polar H10 PMD service.
//
// Connects to the Polar H10, raises the ATT MTU, enables notifications on the
// PMD Data characteristic and indications on the PMD Control Point, then writes
// the "start ECG" command (130 Hz, 14-bit). Incoming ECG frames (int24 LE
// microvolt samples) are parsed and an effective sample rate is reported once
// per second, proving the full sensor -> BLE -> ESP32 path.

#include <string.h>
#include <inttypes.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "esp_psram.h"
#include "esp_heap_caps.h"
#include "nvs_flash.h"

#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "host/ble_gap.h"
#include "host/ble_gatt.h"
#include "host/ble_att.h"
#include "host/util/util.h"

static const char *TAG = "elduro";

// Deterministic handles for the Polar H10 GATT (confirmed via step-1 dump).
#define PMD_CP_VAL    0x002f
#define PMD_CP_CCCD   0x0030
#define PMD_DATA_VAL  0x0032
#define PMD_DATA_CCCD 0x0033

// PMD start-measurement command: ECG, sample rate 130 Hz, resolution 14 bit.
static const uint8_t ECG_START_CMD[] = {
    0x02, 0x00, 0x00, 0x01, 0x82, 0x00, 0x01, 0x01, 0x0E, 0x00,
};

static uint16_t g_conn_handle = BLE_HS_CONN_HANDLE_NONE;

// ECG statistics for the once-per-second report.
static uint64_t g_total_samples = 0;
static uint32_t g_window_samples = 0;
static int64_t g_window_start_us = 0;
static int32_t g_last_sample_uv = 0;

static int gap_event(struct ble_gap_event *event, void *arg);

static void start_scan(void)
{
    uint8_t own_addr_type;
    if (ble_hs_id_infer_auto(0, &own_addr_type) != 0) {
        ESP_LOGE(TAG, "no usable BLE address");
        return;
    }
    struct ble_gap_disc_params disc = { .passive = 0, .filter_duplicates = 1 };
    int rc = ble_gap_disc(own_addr_type, BLE_HS_FOREVER, &disc, gap_event, NULL);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gap_disc failed: %d", rc);
        return;
    }
    ESP_LOGI(TAG, "scanning for a Polar H10 (wear the strap)");
}

static int on_write(uint16_t conn_handle, const struct ble_gatt_error *error,
                    struct ble_gatt_attr *attr, void *arg)
{
    const char *what = (const char *)arg;
    if (error->status != 0) {
        ESP_LOGE(TAG, "%s write failed: status=%d", what, error->status);
        return 0;
    }
    ESP_LOGI(TAG, "%s write OK", what);
    return 0;
}

static void enable_cp_indicate(void);
static void send_ecg_start(void);

static int on_data_cccd(uint16_t conn_handle, const struct ble_gatt_error *error,
                        struct ble_gatt_attr *attr, void *arg)
{
    on_write(conn_handle, error, attr, "data-notify");
    enable_cp_indicate();
    return 0;
}

static int on_cp_cccd(uint16_t conn_handle, const struct ble_gatt_error *error,
                      struct ble_gatt_attr *attr, void *arg)
{
    on_write(conn_handle, error, attr, "cp-indicate");
    send_ecg_start();
    return 0;
}

static void enable_data_notify(void)
{
    static const uint8_t v[2] = { 0x01, 0x00 };
    ble_gattc_write_flat(g_conn_handle, PMD_DATA_CCCD, v, sizeof(v),
                         on_data_cccd, NULL);
}

static void enable_cp_indicate(void)
{
    static const uint8_t v[2] = { 0x02, 0x00 };
    ble_gattc_write_flat(g_conn_handle, PMD_CP_CCCD, v, sizeof(v),
                         on_cp_cccd, NULL);
}

static void send_ecg_start(void)
{
    ESP_LOGI(TAG, "sending start-ECG command to PMD control point");
    ble_gattc_write_flat(g_conn_handle, PMD_CP_VAL, ECG_START_CMD,
                         sizeof(ECG_START_CMD), on_write, "ecg-start");
}

static int on_mtu(uint16_t conn_handle, const struct ble_gatt_error *error,
                  uint16_t mtu, void *arg)
{
    if (error->status == 0) {
        ESP_LOGI(TAG, "MTU negotiated: %d", mtu);
    } else {
        ESP_LOGW(TAG, "MTU exchange status=%d (continuing)", error->status);
    }
    enable_data_notify();
    return 0;
}

static void handle_ecg_frame(const uint8_t *buf, uint16_t len)
{
    // [0]=type(0x00 ECG) [1..8]=timestamp ns [9]=frame type [10..]=int24 LE uV
    if (len < 10 || buf[0] != 0x00) {
        return;
    }
    int nsamp = (len - 10) / 3;
    const uint8_t *p = buf + 10;
    for (int i = 0; i < nsamp; i++) {
        int32_t v = p[0] | (p[1] << 8) | (p[2] << 16);
        if (v & 0x00800000) {
            v |= (int32_t)0xFF000000;  // sign-extend 24-bit
        }
        g_last_sample_uv = v;
        p += 3;
    }
    g_total_samples += nsamp;
    g_window_samples += nsamp;

    int64_t now = esp_timer_get_time();
    if (g_window_start_us == 0) {
        g_window_start_us = now;
        return;
    }
    int64_t dt = now - g_window_start_us;
    if (dt >= 1000000) {
        double hz = (double)g_window_samples * 1e6 / (double)dt;
        ESP_LOGI(TAG,
                 "ECG: %.1f Hz effective, last=%" PRId32 " uV, total=%" PRIu64,
                 hz, g_last_sample_uv, g_total_samples);
        g_window_samples = 0;
        g_window_start_us = now;
    }
}

static int gap_event(struct ble_gap_event *event, void *arg)
{
    switch (event->type) {
    case BLE_GAP_EVENT_DISC: {
        struct ble_hs_adv_fields fields;
        if (ble_hs_adv_parse_fields(&fields, event->disc.data,
                                    event->disc.length_data) != 0) {
            return 0;
        }
        char name[32] = {0};
        if (fields.name != NULL && fields.name_len > 0) {
            int n = fields.name_len < (int)sizeof(name) - 1
                        ? fields.name_len : (int)sizeof(name) - 1;
            memcpy(name, fields.name, n);
        }
        if (strstr(name, "Polar") != NULL) {
            ESP_LOGI(TAG, ">> found '%s', connecting...", name);
            ble_gap_disc_cancel();
            uint8_t own_addr_type;
            ble_hs_id_infer_auto(0, &own_addr_type);
            int rc = ble_gap_connect(own_addr_type, &event->disc.addr, 30000,
                                     NULL, gap_event, NULL);
            if (rc != 0) {
                ESP_LOGE(TAG, "connect init failed: %d; rescanning", rc);
                start_scan();
            }
        }
        return 0;
    }
    case BLE_GAP_EVENT_CONNECT:
        if (event->connect.status == 0) {
            g_conn_handle = event->connect.conn_handle;
            ESP_LOGI(TAG, ">> CONNECTED; raising MTU");
            ble_gattc_exchange_mtu(g_conn_handle, on_mtu, NULL);
        } else {
            ESP_LOGE(TAG, "connect failed status=%d; rescanning",
                     event->connect.status);
            start_scan();
        }
        return 0;
    case BLE_GAP_EVENT_DISCONNECT:
        ESP_LOGW(TAG, "disconnected reason=%d; rescanning",
                 event->disconnect.reason);
        g_conn_handle = BLE_HS_CONN_HANDLE_NONE;
        g_window_start_us = 0;
        start_scan();
        return 0;
    case BLE_GAP_EVENT_NOTIFY_RX: {
        uint16_t h = event->notify_rx.attr_handle;
        uint16_t len = OS_MBUF_PKTLEN(event->notify_rx.om);
        static uint8_t buf[512];
        if (len > sizeof(buf)) {
            len = sizeof(buf);
        }
        ble_hs_mbuf_to_flat(event->notify_rx.om, buf, len, NULL);
        if (h == PMD_DATA_VAL) {
            handle_ecg_frame(buf, len);
        } else if (h == PMD_CP_VAL) {
            ESP_LOGI(TAG, "control-point response len=%d [%02x %02x %02x %02x]",
                     len, len > 0 ? buf[0] : 0, len > 1 ? buf[1] : 0,
                     len > 2 ? buf[2] : 0, len > 3 ? buf[3] : 0);
        }
        return 0;
    }
    default:
        return 0;
    }
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

    ESP_LOGI(TAG, "elduro field bridge - step 2: PMD raw ECG stream");

    ESP_ERROR_CHECK(nimble_port_init());
    ble_att_set_preferred_mtu(247);
    ble_hs_cfg.sync_cb = on_sync;
    ble_hs_cfg.reset_cb = on_reset;
    nimble_port_freertos_init(host_task);

    while (1) {
        vTaskDelay(pdMS_TO_TICKS(15000));
        ESP_LOGI(TAG, "heartbeat: free heap=%u B, free PSRAM=%u B",
                 (unsigned)esp_get_free_heap_size(),
                 (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    }
}
