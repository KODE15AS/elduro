# elduro field bridge (ESP32-S3)

Firmware for the wearable bridge: it links to the Polar H10 over BLE, buffers
raw ECG/ACC/HR, and forwards frames to the elduro backend over WiFi (iPhone
Personal Hotspot in the field). Local store-and-forward to a microSD card is
planned but deferred until the card arrives.

Target board: Seeed XIAO ESP32-S3 Sense (ESP32-S3R8, 8 MB flash, 8 MB octal
PSRAM, native USB-Serial/JTAG).

## Toolchain

Builds run in Docker so nothing lands on the host. Pinned to ESP-IDF v5.4.4.

```sh
IDF=espressif/idf:v5.4.4
FW=$HOME/dev/elduro/firmware
```

## Build

```sh
docker run --rm -v "$FW":/project -w /project "$IDF" \
    idf.py set-target esp32s3 build
```

## Flash

The board is on raven at `/dev/ttyACM0`. Pass the device into the container:

```sh
docker run --rm --device=/dev/ttyACM0 -v "$FW":/project -w /project "$IDF" \
    idf.py -p /dev/ttyACM0 flash
```

USB-Serial/JTAG resets into the bootloader automatically - no BOOT button.

## Monitor

`idf.py monitor` is interactive. For a quick non-interactive log capture after
a reset, read the port directly (pyserial from the esptool venv works):

```sh
docker run --rm --device=/dev/ttyACM0 -it -v "$FW":/project -w /project "$IDF" \
    idf.py -p /dev/ttyACM0 monitor
```

## Current firmware (step 3b)

The full field bridge, driven from the browser:

- WiFi (iPhone hotspot) + secure WebSocket to `wss://elduro.no/ws/agent`;
  registers as the source **ESP32 - Polar H10**.
- BLE link to the Polar H10: raw ECG (130 Hz, int24 uV) + ACC (200 Hz, including
  Polar's delta-compressed frames) + native heart-rate / RR (0x2A37).
- Selecting the source and pressing RECORD / GO LIVE sends
  `{"t":"start","mode":..}`; mode selects the streams: `ecg` = ECG+ACC,
  `hrv` = ECG+ACC+HR, `hr` = HR only. Frames use the same JSON as the USB
  capture agent, so the RAW ECG and RHYTHM/HRV views render them unchanged.
- Amber status LED (GPIO21, active LOW): fast blink = offline (WiFi/WS down),
  slow blink = online/idle, solid = streaming.

WiFi credentials live in `main/wifi_creds.h` (gitignored); copy
`main/wifi_creds.h.example` and fill in your SSID/password. The H10 needs a
~5-35 s warm-up before it emits the first ECG/HR frame after start.

Next steps: PSRAM ring buffer + microSD store-and-forward (FatFs), deferred
until the card arrives; then a second H10 for the dual-sensor stage.
