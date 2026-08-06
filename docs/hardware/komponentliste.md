# Component list (bill of materials)

Field capture unit for the enduro use case: a Seeed Studio XIAO ESP32-S3 Sense
bridges the Polar H10 (BLE PMD raw ECG + ACC) to raven / elduro.no over the
iPhone Personal Hotspot, logging losslessly to microSD. See
[README.md](README.md) for the hardware narrative and power notes, and
[../architecture/field-unit-and-dual-h10-sync.md](../architecture/field-unit-and-dual-h10-sync.md)
for the build plan.

## Ordered parts (Seeed Studio order #4000560622, 2026-07-29)

| # | Part | SKU | Qty | Unit (excl. VAT) | Line | Role |
|---|---|---|---|---|---|---|
| 1 | XIAO ESP32-S3 Sense | 113991115 | 1 | $13.90 | $13.90 | Field-unit MCU: BLE central to H10, WiFi uplink, microSD logging (SD slot on the detachable sensor board) |
| 2 | XIAO ESP32-S3 (plain) | 113991114 | 1 | $7.49 | $7.49 | Bench/dev board and cold spare; no camera/SD |
| 3 | Grove Base for XIAO (battery mgmt) | 103020312 | 1 | $3.90 | $3.90 | LiPo holder + charging + power switch; Grove ports for future GNSS |
| 4 | 2.4GHz FPC Antenna A-02 (1.16 dBi) | 318020968 | 1 | $0.50 | $0.50 | External U.FL antenna for range to the phone |
| | **Subtotal** | | | | **$25.79** | |
| | Shipping (DHL) | | | | $27.85 | |
| | **Grand total** | | | | **$53.64** | |

Order record: [orders/2026-07-29-seeed-order-cart.jpg](orders/2026-07-29-seeed-order-cart.jpg).
VAT 0 % on the invoice (export to NO; import VAT handled separately).

## Key specifications

### XIAO ESP32-S3 Sense (113991115) - primary field unit
- SoC: ESP32-S3R8, Xtensa LX7 dual-core up to 240 MHz.
- Radio: 2.4 GHz Wi-Fi + BLE 5.0 on a single shared radio (see coexistence note).
- Memory: 8 MB PSRAM + 8 MB flash.
- Storage: onboard microSD slot (<= 32 GB, FAT) on the detachable sensor board.
- Daughter board also carries an OV3660 camera (OV2640 on older units) and a
  digital mic - NOT used; never initialize the camera (power draw, no software
  off switch).
- Power (battery, board only): BLE-active ~85 mA, Wi-Fi-active ~100 mA.
- Size 21 x 17.8 mm (x 15 mm with the sensor board); temp -20..65 C.
- U.FL connector for the external FPC antenna.

### XIAO ESP32-S3 plain (113991114) - bench / cold spare
- Same SoC, radio and memory (8 MB PSRAM + 8 MB flash); no camera, mic or SD.
- Deep sleep 14 uA; ships with a U.FL antenna + 2x 7-pin headers.
- Role: development board and a spare if the Sense unit fails.

### Grove Base for XIAO, battery management (103020312)
- LiPo 3.7 V charging + management, power switch, charge-status LED.
- Load capacity 800 mA; charge current up to 400 mA.
- 8 Grove ports (2x IIC, 1x UART); all 14 GPIO broken out; breakable to 25 x 39 mm.
- Charge the battery through this base (~400 mA), not the bare XIAO (~100 mA).

### 2.4GHz FPC Antenna A-02 (318020968)
- Band 2400-2500 MHz, 50 ohm, linear polarization.
- Peak gain ~1.16 dBi (the "16 dBi" on the datasheet cover is a typo), max
  efficiency ~62 %, VSWR <= 4.5:1.
- Flexible FPC with U.FL; fits XIAO ESP32-S3 (Sense).

### LiPo battery, 1S 3.7 V 1000 mAh (sourced locally)
- 1S LiPo, 3.7 V nominal, 1000 mAh, 17 g; 45 x 25 x 8 mm.
- Discharge up to 25C continuous - far above the ESP32-S3's ~350 mA Wi-Fi/SD
  write peaks, so no brown-out sag that could corrupt the SD filesystem.
- White MX2.0 plug matching the 2.0 mm pitch (JST-PH 2.0) on the Grove Base
  pads. VERIFY POLARITY against the Grove Base marking before the first plug-in.
- Mounts with double-sided tape directly under or behind the Grove Base, so the
  whole sensor unit stays compact and wearable.
- Safety: charge only in a fire-safe place; the unit must cut off at >= 3.3 V
  (a cell taken below 3.3 V is permanently ruined); storage voltage 3.8-3.9 V;
  disconnect the battery during storage to avoid sneak drain.
- Charge through the Grove Base (~400 mA), not the bare XIAO.
- Source: modellflybutikken.no (3.7V 1000mAh 1S LiPo).

## Still needed (sourced locally)

- microSD card, <= 32 GB, formatted FAT32 (LiPo now specified above).
- 2.54 mm pin headers (2x 7-pin) to solder the XIAO onto the Grove Base
  (14 joints).
- Enclosure / strap mount for the chest-worn unit.
- Second Polar H10 for the dual-belt AV-block stage (not yet ordered).

## Datasheets

Vendored under [datasheets/](datasheets/) (fetched 2026-07-29). Do not rename
the committed files.

| File | Document |
|---|---|
| [seeed-xiao-esp32s3-sense-sku113991115.pdf](datasheets/seeed-xiao-esp32s3-sense-sku113991115.pdf) | XIAO ESP32-S3 Sense |
| [seeed-xiao-esp32s3-sku113991114.pdf](datasheets/seeed-xiao-esp32s3-sku113991114.pdf) | XIAO ESP32-S3 (plain) |
| [seeed-grove-base-for-xiao-sku103020312.pdf](datasheets/seeed-grove-base-for-xiao-sku103020312.pdf) | Grove Base for XIAO |
| [seeed-fpc-antenna-a02-2g4-sku318020968.pdf](datasheets/seeed-fpc-antenna-a02-2g4-sku318020968.pdf) | 2.4GHz FPC Antenna A-02 |

Worth adding later: the Espressif ESP32-S3 chip datasheet and the Polar PMD
service spec from the Polar BLE SDK - the most important protocol document in
the project.

## Coexistence note

The ESP32-S3 has ONE 2.4 GHz radio shared by BLE and Wi-Fi. Running H10 BLE
capture and the Wi-Fi uplink together relies on microSD store-and-forward so a
busy radio never means lost data. In the dual-H10 stage (2x BLE + Wi-Fi) this
matters more; the build plan handles it by keeping synchronization in
post-processing on the device clocks.
