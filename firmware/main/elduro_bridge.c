// elduro field bridge - step 3b: Polar H10 (BLE) -> ESP32 -> backend (WiFi/WS).
//
// The full field bridge, driven by the browser:
//   * WiFi + secure WebSocket to wss://elduro.no/ws/agent (registers as a source)
//   * NimBLE link to the Polar H10: raw ECG @130 Hz + ACC @200 Hz (PMD service)
//     and native heart-rate / RR intervals (standard 0x2A37 characteristic)
//   * amber status LED on GPIO21 (active LOW)
//
// Selecting the "ESP32 - Polar H10" source and pressing RECORD / GO LIVE sends
// {"t":"start","mode":..} down the agent WS. Mode decides which streams run:
//   ecg -> ECG + ACC          hrv -> ECG + ACC + HR/RR          hr -> HR/RR only
// Frames are forwarded in the exact JSON the USB agent emits, so the existing
// RAW ECG / RHYTHM-HRV views render them unchanged.
//
// LED: fast blink = offline (WiFi/WS down), slow blink = online/idle,
//      solid = streaming to the backend.

#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <inttypes.h>

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "freertos/event_groups.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_mac.h"
#include "esp_crt_bundle.h"
#include "esp_websocket_client.h"
#include "driver/gpio.h"
#include "nvs_flash.h"

#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "host/ble_gap.h"
#include "host/ble_gatt.h"
#include "host/ble_att.h"
#include "host/ble_uuid.h"
#include "host/util/util.h"

#include "wifi_creds.h"

static const char *TAG = "elduro";

#define LED_GPIO      GPIO_NUM_21
#define GOT_IP_BIT    BIT0

// Deterministic Polar H10 PMD GATT handles (confirmed via step-1 dump). The
// HR Measurement handle varies, so that one is discovered by UUID at connect.
#define PMD_CP_VAL    0x002f
#define PMD_CP_CCCD   0x0030
#define PMD_DATA_VAL  0x0032
#define PMD_DATA_CCCD 0x0033
#define HRM_UUID16    0x2A37

static const uint8_t ECG_START_CMD[] = {
    0x02, 0x00, 0x00, 0x01, 0x82, 0x00, 0x01, 0x01, 0x0e, 0x00,
};
static const uint8_t ACC_START_CMD[] = {
    0x02, 0x02, 0x00, 0x01, 0xc8, 0x00, 0x01, 0x01, 0x10, 0x00,
    0x02, 0x01, 0x08, 0x00,
};
static const uint8_t ECG_STOP_CMD[] = { 0x03, 0x00 };
static const uint8_t ACC_STOP_CMD[] = { 0x03, 0x02 };

#define ECG_PERIOD_NS (1000000000ULL / 130)

typedef enum { MODE_ECG, MODE_HRV, MODE_HR } stream_mode_t;
typedef enum { LED_OFFLINE, LED_ONLINE, LED_STREAM } led_state_t;

static volatile led_state_t g_led = LED_OFFLINE;

static EventGroupHandle_t s_wifi_events;
static int s_retries = 0;
static char s_agent_id[32];
static char s_source[40];
static esp_websocket_client_handle_t s_ws;
static volatile bool g_ws_up = false;

static uint16_t g_conn = BLE_HS_CONN_HANDLE_NONE;
static uint16_t g_hr_val = 0;               // HR Measurement value handle (0 = not found)
static volatile bool g_ble_ready = false;
static volatile bool g_want_stream = false;
static volatile bool g_streaming = false;
static volatile stream_mode_t g_mode = MODE_ECG;

static QueueHandle_t s_tx_queue;

static int64_t g_session_start_us = 0;
static uint64_t g_ecg_total = 0;
static uint64_t g_acc_total = 0;
static uint32_t g_gaps = 0;
static uint64_t g_prev_ecg_ts = 0;
static bool g_have_prev_ecg = false;

static void led_refresh(void)
{
    g_led = !g_ws_up ? LED_OFFLINE : (g_streaming ? LED_STREAM : LED_ONLINE);
}

// ---- status LED ------------------------------------------------------------

static void led_task(void *arg)
{
    gpio_config_t io = { .pin_bit_mask = 1ULL << LED_GPIO, .mode = GPIO_MODE_OUTPUT };
    gpio_config(&io);
    while (1) {
        led_state_t s = g_led;
        if (s == LED_STREAM) {
            gpio_set_level(LED_GPIO, 0);
            vTaskDelay(pdMS_TO_TICKS(100));
            continue;
        }
        int period = (s == LED_ONLINE) ? 500 : 100;
        gpio_set_level(LED_GPIO, 0);
        vTaskDelay(pdMS_TO_TICKS(period));
        gpio_set_level(LED_GPIO, 1);
        vTaskDelay(pdMS_TO_TICKS(period));
    }
}

// ---- frame queue + WS sender ----------------------------------------------

static void enqueue(char *json)
{
    if (!json) return;
    if (!g_ws_up || xQueueSend(s_tx_queue, &json, 0) != pdTRUE) {
        free(json);
    }
}

static void ws_sender_task(void *arg)
{
    char *json;
    while (1) {
        if (xQueueReceive(s_tx_queue, &json, portMAX_DELAY) == pdTRUE) {
            if (g_ws_up) {
                esp_websocket_client_send_text(s_ws, json, strlen(json),
                                               pdMS_TO_TICKS(1000));
            }
            free(json);
        }
    }
}

// ---- PMD / HR control ------------------------------------------------------

static void hr_enable(bool on)
{
    if (!g_hr_val || g_conn == BLE_HS_CONN_HANDLE_NONE) return;
    uint8_t v[2] = { (uint8_t)(on ? 0x01 : 0x00), 0x00 };  // notifications
    ble_gattc_write_flat(g_conn, g_hr_val + 1, v, sizeof(v), NULL, NULL);
}

static void start_measurements(void)
{
    if (g_streaming || !g_ble_ready) return;
    g_session_start_us = esp_timer_get_time();
    g_ecg_total = g_acc_total = 0;
    g_gaps = 0;
    g_have_prev_ecg = false;
    bool want_ecg = (g_mode == MODE_ECG || g_mode == MODE_HRV);
    bool want_hr  = (g_mode == MODE_HRV || g_mode == MODE_HR);
    if (want_ecg) {
        ble_gattc_write_flat(g_conn, PMD_CP_VAL, ECG_START_CMD, sizeof(ECG_START_CMD), NULL, NULL);
        ble_gattc_write_flat(g_conn, PMD_CP_VAL, ACC_START_CMD, sizeof(ACC_START_CMD), NULL, NULL);
    }
    if (want_hr) hr_enable(true);
    g_streaming = true;
    led_refresh();
    ESP_LOGI(TAG, ">> streaming (mode=%d ecg=%d hr=%d)", g_mode, want_ecg, want_hr);
}

static void stop_measurements(void)
{
    if (g_ble_ready && g_conn != BLE_HS_CONN_HANDLE_NONE) {
        ble_gattc_write_flat(g_conn, PMD_CP_VAL, ECG_STOP_CMD, sizeof(ECG_STOP_CMD), NULL, NULL);
        ble_gattc_write_flat(g_conn, PMD_CP_VAL, ACC_STOP_CMD, sizeof(ACC_STOP_CMD), NULL, NULL);
        hr_enable(false);
    }
    g_streaming = false;
    led_refresh();
    ESP_LOGI(TAG, ">> streaming stopped");
}

// ---- WebSocket agent -------------------------------------------------------

static void send_register(void)
{
    char msg[160];
    int n = snprintf(msg, sizeof(msg),
        "{\"t\":\"register\",\"agent\":\"%s\","
        "\"adapters\":[{\"id\":\"h10\",\"label\":\"ESP32 Polar H10\"}]}",
        s_agent_id);
    esp_websocket_client_send_text(s_ws, msg, n, pdMS_TO_TICKS(2000));
    ESP_LOGI(TAG, "registered as %s", s_agent_id);
}

static void handle_command(const char *data, int len)
{
    if (strnstr(data, "\"t\":\"start\"", len)) {
        if (strnstr(data, "\"mode\":\"hrv\"", len)) g_mode = MODE_HRV;
        else if (strnstr(data, "\"mode\":\"hr\"", len)) g_mode = MODE_HR;
        else g_mode = MODE_ECG;
        ESP_LOGI(TAG, "cmd: start (mode=%d)", g_mode);
        g_want_stream = true;
        if (g_ble_ready) start_measurements();
    } else if (strnstr(data, "\"t\":\"stop\"", len)) {
        ESP_LOGI(TAG, "cmd: stop");
        g_want_stream = false;
        stop_measurements();
    }
}

static void ws_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    esp_websocket_event_data_t *e = (esp_websocket_event_data_t *)data;
    switch (id) {
    case WEBSOCKET_EVENT_CONNECTED:
        ESP_LOGI(TAG, ">> WS connected");
        g_ws_up = true;
        send_register();
        led_refresh();
        break;
    case WEBSOCKET_EVENT_DATA:
        if (e->op_code == 0x1 && e->data_len > 0) {
            handle_command((const char *)e->data_ptr, e->data_len);
        }
        break;
    case WEBSOCKET_EVENT_DISCONNECTED:
    case WEBSOCKET_EVENT_ERROR:
        ESP_LOGW(TAG, "WS down");
        g_ws_up = false;
        led_refresh();
        break;
    default:
        break;
    }
}

static void ws_start(void)
{
    esp_websocket_client_config_t cfg = {
        .uri = "wss://elduro.no/ws/agent",
        .crt_bundle_attach = esp_crt_bundle_attach,
        .reconnect_timeout_ms = 5000,
        .network_timeout_ms = 10000,
        .buffer_size = 4096,
    };
    s_ws = esp_websocket_client_init(&cfg);
    esp_websocket_register_events(s_ws, WEBSOCKET_EVENT_ANY, ws_event, NULL);
    esp_websocket_client_start(s_ws);
    ESP_LOGI(TAG, "WS -> %s", cfg.uri);
}

// ---- ECG / ACC / HR frame building -----------------------------------------

static void emit_ecg(const uint8_t *buf, uint16_t len)
{
    if (len < 10 || buf[9] != 0x00) return;
    uint64_t ts = 0;
    for (int i = 0; i < 8; i++) ts |= (uint64_t)buf[1 + i] << (8 * i);
    int nsamp = (len - 10) / 3;
    if (nsamp <= 0) return;

    if (g_have_prev_ecg) {
        uint64_t expected = (uint64_t)nsamp * ECG_PERIOD_NS;
        if (ts - g_prev_ecg_ts > expected + ECG_PERIOD_NS) g_gaps++;
    }
    g_prev_ecg_ts = ts;
    g_have_prev_ecg = true;
    g_ecg_total += nsamp;

    int64_t now = esp_timer_get_time();
    uint64_t elapsed_ms = (now - g_session_start_us) / 1000;
    uint64_t host_ns = (uint64_t)now * 1000;

    size_t cap = 220 + (size_t)nsamp * 9;
    char *out = malloc(cap);
    if (!out) return;
    int p = snprintf(out, cap,
        "{\"t\":\"ecg\",\"source\":\"%s\",\"ts_device_ns\":%" PRIu64
        ",\"ts_host_ns\":%" PRIu64 ",\"elapsed_ms\":%" PRIu64 ",\"samples\":[",
        s_source, ts, host_ns, elapsed_ms);
    const uint8_t *s = buf + 10;
    for (int i = 0; i < nsamp; i++) {
        int32_t v = s[0] | (s[1] << 8) | (s[2] << 16);
        if (v & 0x00800000) v |= (int32_t)0xFF000000;
        p += snprintf(out + p, cap - p, i ? ",%" PRId32 : "%" PRId32, v);
        s += 3;
    }
    snprintf(out + p, cap - p, "],\"total\":%" PRIu64 ",\"gaps\":%" PRIu32 "}",
             g_ecg_total, g_gaps);
    enqueue(out);
}

static int32_t sign_ext(uint32_t v, int bits)
{
    if (bits <= 0 || bits >= 32) return (int32_t)v;
    int sh = 32 - bits;
    return ((int32_t)(v << sh)) >> sh;
}
static uint32_t read_bits(const uint8_t *d, int dlen, int start_bit, int n)
{
    uint32_t val = 0;
    for (int i = 0; i < n; i++) {
        int bp = start_bit + i;
        if (bp / 8 >= dlen) break;
        val |= (uint32_t)((d[bp / 8] >> (bp % 8)) & 1) << i;
    }
    return val;
}

static void emit_acc(const uint8_t *buf, uint16_t len)
{
    if (len < 10) return;
    uint64_t ts = 0;
    for (int i = 0; i < 8; i++) ts |= (uint64_t)buf[1 + i] << (8 * i);
    uint8_t frame_type = buf[9];
    const uint8_t *body = buf + 10;
    int blen = len - 10;
    bool compressed = (frame_type & 0x80) != 0;

    static int32_t tri[128][3];
    int ntri = 0;

    if (!compressed) {
        for (int off = 0; off + 6 <= blen && ntri < 128; off += 6) {
            tri[ntri][0] = (int16_t)(body[off] | (body[off + 1] << 8));
            tri[ntri][1] = (int16_t)(body[off + 2] | (body[off + 3] << 8));
            tri[ntri][2] = (int16_t)(body[off + 4] | (body[off + 5] << 8));
            ntri++;
        }
    } else {
        if (blen < 6) return;
        int32_t cur[3] = {
            (int16_t)(body[0] | (body[1] << 8)),
            (int16_t)(body[2] | (body[3] << 8)),
            (int16_t)(body[4] | (body[5] << 8)),
        };
        tri[ntri][0] = cur[0]; tri[ntri][1] = cur[1]; tri[ntri][2] = cur[2];
        ntri++;
        int pos = 6;
        while (pos + 2 <= blen && ntri < 128) {
            int delta_size = body[pos];
            int count = body[pos + 1];
            pos += 2;
            if (delta_size == 0 || count == 0) break;
            int total = count * 3;
            int bit = 0, base = pos * 8;
            for (int v = 0; v < total && ntri < 128; v++) {
                uint32_t raw = read_bits(body, blen, base + bit, delta_size);
                cur[v % 3] += sign_ext(raw, delta_size);
                if (v % 3 == 2) {
                    tri[ntri][0] = cur[0]; tri[ntri][1] = cur[1]; tri[ntri][2] = cur[2];
                    ntri++;
                }
                bit += delta_size;
            }
            pos += (bit + 7) / 8;
        }
    }
    if (ntri == 0) return;
    g_acc_total += ntri;

    uint64_t host_ns = (uint64_t)esp_timer_get_time() * 1000;
    size_t cap = 160 + (size_t)ntri * 22;
    char *out = malloc(cap);
    if (!out) return;
    int p = snprintf(out, cap,
        "{\"t\":\"acc\",\"source\":\"%s\",\"ts_device_ns\":%" PRIu64
        ",\"ts_host_ns\":%" PRIu64 ",\"samples\":[", s_source, ts, host_ns);
    for (int i = 0; i < ntri; i++) {
        p += snprintf(out + p, cap - p, i ? ",[%" PRId32 ",%" PRId32 ",%" PRId32 "]"
                                          : "[%" PRId32 ",%" PRId32 ",%" PRId32 "]",
                      tri[i][0], tri[i][1], tri[i][2]);
    }
    snprintf(out + p, cap - p, "],\"total\":%" PRIu64 "}", g_acc_total);
    enqueue(out);
}

// Standard Heart Rate Measurement (0x2A37): flags, 8/16-bit BPM, optional RR.
static void emit_hr(const uint8_t *d, int len)
{
    if (len < 2) return;
    uint8_t flags = d[0];
    int i = 1;
    uint16_t bpm;
    if (flags & 0x01) {
        if (len < 3) return;
        bpm = d[1] | (d[2] << 8);
        i = 3;
    } else {
        bpm = d[1];
        i = 2;
    }
    if (flags & 0x08) i += 2;  // energy expended field

    int64_t now = esp_timer_get_time();
    uint64_t elapsed_ms = (now - g_session_start_us) / 1000;
    char *out = malloc(256);
    if (!out) return;
    int p = snprintf(out, 256,
        "{\"t\":\"hr\",\"source\":\"%s\",\"ts\":%" PRIu64 ",\"bpm\":%u,\"rr\":[",
        s_source, elapsed_ms, bpm);
    bool first = true;
    if (flags & 0x10) {
        while (i + 2 <= len) {
            uint16_t raw = d[i] | (d[i + 1] << 8);
            uint32_t ms = (uint32_t)raw * 1000 / 1024;  // 1/1024 s -> ms
            p += snprintf(out + p, 256 - p, first ? "%" PRIu32 : ",%" PRIu32, ms);
            first = false;
            i += 2;
        }
    }
    snprintf(out + p, 256 - p, "]}");
    enqueue(out);
}

// ---- NimBLE ----------------------------------------------------------------

static int gap_event(struct ble_gap_event *event, void *arg);

static void start_scan(void)
{
    uint8_t own;
    if (ble_hs_id_infer_auto(0, &own) != 0) return;
    struct ble_gap_disc_params disc = { .passive = 0, .filter_duplicates = 1 };
    ble_gap_disc(own, BLE_HS_FOREVER, &disc, gap_event, NULL);
    ESP_LOGI(TAG, "scanning for Polar H10 (wear the strap)");
}

static int on_hrm_disc(uint16_t ch, const struct ble_gatt_error *err,
                       const struct ble_gatt_chr *chr, void *arg)
{
    if (err->status == 0 && chr) {
        g_hr_val = chr->val_handle;
    } else {
        // BLE_HS_EDONE (or error): discovery finished; arm the link.
        g_ble_ready = true;
        ESP_LOGI(TAG, ">> H10 armed (hr_val=0x%04x); %s",
                 g_hr_val, g_want_stream ? "starting" : "idle");
        if (g_want_stream) start_measurements();
    }
    return 0;
}

static int on_cp_cccd(uint16_t ch, const struct ble_gatt_error *err,
                      struct ble_gatt_attr *attr, void *arg)
{
    static const ble_uuid16_t hrm = BLE_UUID16_INIT(HRM_UUID16);
    if (ble_gattc_disc_chrs_by_uuid(g_conn, 1, 0xffff, &hrm.u, on_hrm_disc, NULL) != 0) {
        g_ble_ready = true;
        if (g_want_stream) start_measurements();
    }
    return 0;
}
static int on_data_cccd(uint16_t ch, const struct ble_gatt_error *err,
                        struct ble_gatt_attr *attr, void *arg)
{
    static const uint8_t v[2] = { 0x02, 0x00 };  // indications on PMD control
    ble_gattc_write_flat(g_conn, PMD_CP_CCCD, v, sizeof(v), on_cp_cccd, NULL);
    return 0;
}
static int on_mtu(uint16_t ch, const struct ble_gatt_error *err, uint16_t mtu, void *arg)
{
    ESP_LOGI(TAG, "MTU=%d", mtu);
    static const uint8_t v[2] = { 0x01, 0x00 };  // notifications on PMD data
    ble_gattc_write_flat(g_conn, PMD_DATA_CCCD, v, sizeof(v), on_data_cccd, NULL);
    return 0;
}

static int gap_event(struct ble_gap_event *event, void *arg)
{
    switch (event->type) {
    case BLE_GAP_EVENT_DISC: {
        struct ble_hs_adv_fields f;
        if (ble_hs_adv_parse_fields(&f, event->disc.data, event->disc.length_data) != 0)
            return 0;
        char name[32] = {0};
        if (f.name && f.name_len > 0) {
            int n = f.name_len < 31 ? f.name_len : 31;
            memcpy(name, f.name, n);
        }
        if (strstr(name, "Polar")) {
            ESP_LOGI(TAG, ">> found '%s', connecting", name);
            ble_gap_disc_cancel();
            uint8_t own;
            ble_hs_id_infer_auto(0, &own);
            if (ble_gap_connect(own, &event->disc.addr, 30000, NULL, gap_event, NULL) != 0)
                start_scan();
        }
        return 0;
    }
    case BLE_GAP_EVENT_CONNECT:
        if (event->connect.status == 0) {
            g_conn = event->connect.conn_handle;
            ESP_LOGI(TAG, ">> H10 connected; raising MTU");
            ble_gattc_exchange_mtu(g_conn, on_mtu, NULL);
        } else {
            start_scan();
        }
        return 0;
    case BLE_GAP_EVENT_DISCONNECT:
        ESP_LOGW(TAG, "H10 disconnected reason=%d", event->disconnect.reason);
        g_conn = BLE_HS_CONN_HANDLE_NONE;
        g_hr_val = 0;
        g_ble_ready = false;
        g_streaming = false;
        led_refresh();
        start_scan();
        return 0;
    case BLE_GAP_EVENT_NOTIFY_RX: {
        uint16_t h = event->notify_rx.attr_handle;
        uint16_t len = OS_MBUF_PKTLEN(event->notify_rx.om);
        static uint8_t buf[512];
        if (len > sizeof(buf)) len = sizeof(buf);
        ble_hs_mbuf_to_flat(event->notify_rx.om, buf, len, NULL);
        if (h == PMD_DATA_VAL && len >= 1) {
            if (buf[0] == 0x00) emit_ecg(buf, len);
            else if (buf[0] == 0x02) emit_acc(buf, len);
        } else if (g_hr_val && h == g_hr_val) {
            emit_hr(buf, len);
        }
        return 0;
    }
    default:
        return 0;
    }
}

static void on_sync(void) { ble_hs_util_ensure_addr(0); start_scan(); }
static void on_reset(int reason) { ESP_LOGW(TAG, "BLE reset %d", reason); }
static void host_task(void *param) { nimble_port_run(); nimble_port_freertos_deinit(); }

// ---- WiFi ------------------------------------------------------------------

static void wifi_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        g_ws_up = false;
        led_refresh();
        if (s_retries++ < 1000) esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = (ip_event_got_ip_t *)data;
        ESP_LOGI(TAG, ">> GOT IP: " IPSTR, IP2STR(&e->ip_info.ip));
        s_retries = 0;
        xEventGroupSetBits(s_wifi_events, GOT_IP_BIT);
    }
}

static void net_task(void *param)
{
    s_wifi_events = xEventGroupCreate();
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_sta();

    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&init));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(WIFI_EVENT, ESP_EVENT_ANY_ID,
                                                        wifi_event, NULL, NULL));
    ESP_ERROR_CHECK(esp_event_handler_instance_register(IP_EVENT, IP_EVENT_STA_GOT_IP,
                                                        wifi_event, NULL, NULL));

    uint8_t mac[6];
    esp_wifi_get_mac(WIFI_IF_STA, mac);
    snprintf(s_agent_id, sizeof(s_agent_id), "esp32-%02x%02x%02x%02x%02x%02x",
             mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    snprintf(s_source, sizeof(s_source), "%s:h10", s_agent_id);

    wifi_config_t wc = { 0 };
    strncpy((char *)wc.sta.ssid, WIFI_SSID, sizeof(wc.sta.ssid) - 1);
    strncpy((char *)wc.sta.password, WIFI_PASS, sizeof(wc.sta.password) - 1);
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wc));
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_LOGI(TAG, "connecting to '%s'", WIFI_SSID);
    esp_wifi_connect();

    xEventGroupWaitBits(s_wifi_events, GOT_IP_BIT, pdFALSE, pdTRUE, portMAX_DELAY);
    ws_start();
    vTaskDelete(NULL);
}

void app_main(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ESP_ERROR_CHECK(nvs_flash_init());
    }
    ESP_LOGI(TAG, "elduro field bridge - step 3b: H10 (ECG+ACC+HR) -> WiFi/WS");

    s_tx_queue = xQueueCreate(48, sizeof(char *));
    xTaskCreate(led_task, "led", 4096, NULL, 4, NULL);
    xTaskCreate(ws_sender_task, "wstx", 8192, NULL, 6, NULL);
    xTaskCreate(net_task, "net", 8192, NULL, 5, NULL);

    ESP_ERROR_CHECK(nimble_port_init());
    ble_att_set_preferred_mtu(247);
    ble_hs_cfg.sync_cb = on_sync;
    ble_hs_cfg.reset_cb = on_reset;
    nimble_port_freertos_init(host_task);
}
