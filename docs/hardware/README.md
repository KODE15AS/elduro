# Hardware

Field capture unit for the enduro use case: a XIAO ESP32-S3 Sense acts as a
BLE-to-WiFi bridge, connecting to the Polar H10 over BLE (PMD raw ECG + ACC),
recording losslessly to microSD, and streaming to raven via the iPhone's
Personal Hotspot. The raven ASUS BT-600 setup remains the bench reference for
verification.

## Inventory

Full bill of materials - parts, SKUs, quantities, prices, order
reconciliation and the locally-sourced items still needed - is in
[komponentliste.md](komponentliste.md).

Order record: [orders/2026-07-29-seeed-order-cart.jpg](orders/2026-07-29-seeed-order-cart.jpg)

## Datasheets

Vendored PDFs, fetched 2026-07-29 from the Seeed product pages.
Do not rename committed files.

| File | Document | Source |
|---|---|---|
| [seeed-xiao-esp32s3-sense-sku113991115.pdf](datasheets/seeed-xiao-esp32s3-sense-sku113991115.pdf) | XIAO ESP32-S3 Sense product datasheet | [product page](https://www.seeedstudio.com/XIAO-ESP32S3-Sense-p-5639.html) |
| [seeed-xiao-esp32s3-sku113991114.pdf](datasheets/seeed-xiao-esp32s3-sku113991114.pdf) | XIAO ESP32-S3 (plain) product datasheet | [product page](https://www.seeedstudio.com/XIAO-ESP32S3-p-5627.html) |
| [seeed-grove-base-for-xiao-sku103020312.pdf](datasheets/seeed-grove-base-for-xiao-sku103020312.pdf) | Grove Base for XIAO with battery management | [product page](https://www.seeedstudio.com/Grove-Shield-for-Seeeduino-XIAO-p-4621.html) |
| [seeed-fpc-antenna-a02-2g4-sku318020968.pdf](datasheets/seeed-fpc-antenna-a02-2g4-sku318020968.pdf) | 2.4GHz FPC Antenna A-02 | [product page](https://www.seeedstudio.com/2-4GHz-FPC-Antenna-1-16dBi-for-XIAO-ESP32S3-p-6440.html) |

Worth adding later: Espressif ESP32-S3 chip datasheet (sleep currents, RF
specs) and the Polar PMD service spec from the Polar BLE SDK repo - the most
important protocol document in the project.

## Power notes

- Active streaming (WiFi + BLE) draws roughly 100-140 mA at 3.8 V, so a
  1000 mAh LiPo gives 7-9 hours; sleep-mode figures are irrelevant while
  streaming.
- Sense camera daughter board: ~3 mA extra when the camera is left
  uninitialized, ~90 mA if firmware ever initializes it, with no software
  power-off. Never initialize the camera. The microSD slot is on the same
  daughter board, so keep the board attached.
- ESP32 WiFi is 2.4 GHz only: the iPhone Personal Hotspot must have
  "Maximize Compatibility" enabled.
- Charge the battery through the Grove Base (400-500 mA max) rather than the
  bare XIAO (~100 mA).
