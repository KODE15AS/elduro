# USB-lagring på feltbroen – vurdering (20.09.2026)

Bakgrunn: reservekortet (XIAO ESP32-S3 **plain**) har ikke microSD-sporet
(det sitter på Sense-datterkortet, nytt bestilt). Idé fra Jørn: bruke en
**«2-i-1 SSD-powerbank»** (Movespeed PowerDisk, ADATA SR800 e.l.) så vi får
strøm + lagring i én kompakt enhet på nåværende kort.

## Konklusjon først

**SSD-powerbanken passer ikke til ESP32-en – feil verktøy for jobben.** Men
det underliggende målet (lagring på reservekortet) er oppnåelig med en enkel
**bus-drevet USB-minnepinne** via ESP32-S3-ens USB-MSC-vertsmodus. Anbefaling:
ikke kjøp SSD-powerbank; vent heller på det nye Sense-kortet (microSD er den
rene, kompakte, allerede implementerte veien), eller — hvis vi vil ha lagring
på reservekortet nå — bygg den lille USB-minnepinne-løsningen under.

## Hvorfor SSD-powerbanken ikke fungerer

1. **Rollekonflikt på VBUS.** En 2-i-1 SSD-powerbank er laget for å være
   *periferienhet* til en telefon/PC: telefonen er USB-**vert**, og powerbanken
   både lader telefonen og viser SSD-en til den. Powerbanken *leverer* 5 V på
   VBUS. For at ESP32-en skal lese SSD-en må ESP32-en være **verten**, og en
   USB-vert må selv *levere* 5 V VBUS til enheten. To strømkilder på samme buss
   = konflikt. Powerbanken forventer å drive verten, ikke bli drevet av den.
2. **Hastighet og protokoll.** ESP32-S3 er USB 2.0 **full-speed (12 Mbps)** og
   støtter kun MSC BOT (Bulk-Only Transport). SSD-powerbankene er USB 3.2
   UASP/NVMe (opptil 2000 MB/s). ESP32-en ville uansett falt tilbake til
   ~1 MB/s, og UASP/NVMe støttes ikke. (Vår datarate er ~20 kB/s, så fart er
   ikke poenget – men det viser at en dyr NVMe-SSD er bortkastet her.)
3. **Kapasitet vs. FAT32-beslutningen.** Vår frosne beslutning er FAT32 ≤ 32 GB.
   SSD-powerbankene er 512 GB–2 TB og må da kjøres exFAT (lisens-/patent-noter i
   ESP-IDF FatFs) eller FAT32 via spesialverktøy på PC. Nok en grunn til å holde
   seg til ≤ 32 GB.

## Hva som FAKTISK virker på ESP32-S3 (kilder: ESP-IDF USB-MSC-host + Seeed-forum)

ESP32-S3 kan være **USB-MSC-vert** for en FAT-formatert USB-minnepinne
(ESP-IDF `usb_host` + `msc_host`, eksempel `peripherals/usb/host/msc`,
monteres på `/usb0`). Forutsetninger og fallgruver:

- **Ekstern 5 V på VBus-pinnen kreves.** XIAO-ens LiPo-grensesnitt leverer
  IKKE VBUS til vertsperiferienheter. Uten 5 V på bussen «skjer ingenting»
  (enheten enumereres ikke). Løsning: 1S LiPo + 5 V-boost-modul (IP5306/
  FM5324GA-basert) inn på VBus (7-pin/J1), som også lader batteriet.
- **Én USB-C-port kan ikke være både PC-debug og vert samtidig.** I vertsmodus
  brukes D+/D− (GPIO19/20); den native USB-Serial/JTAG-en er av. Debug flyttes
  til UART på D6/D7. Firmware-flash må da skje via UART-adapter, ikke USB-C.
- Vert- og enhetsmodus er gjensidig utelukkende (bytte i programvare).

Dette er altså mulig, men **mer kompleks og større** enn microSD: ekstra
boost-modul, tapt native-USB (tungvint flashing), og en firmware-port fra
SDSPI/FatFs til USB-MSC-host. Nytten er marginal siden Sense-kortets microSD
gir samme lagring, mer kompakt, uten disse ulempene.

## Konkret startpunkt (hvis vi likevel vil teste USB-lagring på reservekortet)

Minst mulig, billigst mulig, ≤ 32 GB:

1. **USB-minnepinne:** SanDisk Ultra Fit 32 GB (USB 3.2, men bus-drevet og
   bakoverkompatibel USB 2.0; ~2 cm, stikker knapt ut). Formateres **FAT32**
   rett på PC – 32 GB krever ingen spesialverktøy (Windows' innebygde format
   tar 32 GB FAT32). ~100 kr.
2. **OTG/kabling:** USB-C OTG-adapter, eller loddet D+/D− til GPIO19/20.
3. **Strøm/VBUS:** 1S LiPo (1000 mAh, som allerede planlagt) + liten
   IP5306-basert 5 V-boost/lade-modul inn på VBus – gir både drift, VBUS til
   minnepinnen og lading i én liten pakke. ~50–100 kr.

Firmware: port `sd_*`-laget fra SDSPI/FatFs til `usb_host`+`msc_host` (monter
`/usb0` i stedet for `/sdcard`); resten av spill-logikken (øktkataloger,
`frames.jsonl`, `seq`, fsync) er uendret.

## Anbefaling (til beslutning – norm «bestilling»)

1. **Primært:** vent på nytt XIAO ESP32-S3 **Sense**-kort (allerede bestilt) →
   microSD er den rene feltløsningen. Ingen nye innkjøp.
2. **Kun hvis vi vil ha lagring på reservekortet i mellomtiden:** kjøp den lille
   USB-minnepinne + boost-modul-løsningen over (~150–200 kr). Dette er en
   innkjøpsbeslutning – jeg bestiller ikke; si fra om du vil ha den.
3. **Ikke kjøp** en 2-i-1 SSD-powerbank til dette formålet.
