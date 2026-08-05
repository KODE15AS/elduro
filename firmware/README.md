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

## Current firmware

Connects to the Polar H10, raises the ATT MTU, enables notifications on the PMD
Data characteristic and indications on the PMD Control Point, writes the
start-ECG command (130 Hz, 14-bit), and parses the incoming int24 microvolt
samples, reporting an effective sample rate once per second. Verified on
hardware at ~130 Hz.

Next steps: capture ACC alongside ECG, add a PSRAM ring buffer, and forward
frames over WiFi to the backend. microSD store-and-forward (FatFs) is deferred
until the card arrives.
