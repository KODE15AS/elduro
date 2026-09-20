# Alternative CPU-er til ESP32-S3 for feltbroen – vurdering (20.09.2026)

Bakgrunn: dagens feltbro er XIAO ESP32-S3. Vår største smertekilde har vært
**BLE + WiFi på én delt 2,4 GHz-radio** (coex-ustabilitet, 531-stormer,
antenne-følsomhet). Etter fiksene (BLE-prioritet under strømming, backoff,
selvhelbredelse) fungerer ESP32-S3 nå. Spørsmålet: finnes det små CPU-er som
fjerner denne problemklassen, eller ellers passer bedre?

## Kravene våre

- BLE-**sentral** mot Polar H10 (PMD: EKG 130 Hz, ACC 200 Hz, HR 0x2A37).
- WiFi-oppkobling (iPhone-hotspot → `wss://elduro.no`). WiFi er et krav –
  derfor forkastet vi rene BLE-brikker (XIAO nRF52840) i chat 2.
- Lagring (microSD store-and-forward), liten/bærbar, 1S 1000 mAh en økt.
- Native USB for flashing er en bonus.

## Konklusjon først

**Ikke bytt reaktivt – ESP32-S3 virker nå.** Men to alternativer er verdt å
kjenne til: (1) **Raspberry Pi Zero 2 W** gir størst gevinst med minst
firmware-risiko fordi den kan kjøre *nøyaktig* vår eksisterende Rust
capture-agent (btleplug/BlueZ) – null portering – mot en kostnad i strøm/
størrelse; (2) **nRF5340 + nRF7002** er den beste RF-arkitekturen (dedikert
BLE-radio + 3-tråds coex), men krever Zephyr + tilpasset kort = høy innsats.

## Kandidatene

### A. XIAO ESP32-C6 (sidegrade, lav innsats)
- WiFi 6 + BLE 5.3, samme lille XIAO-formfaktor, ESP-IDF – nesten drop-in.
- **Men fortsatt én delt 2,4 GHz-radio** → løser ikke coex-problemklassen.
  WiFi 6 Target Wake Time kan gi litt bedre strøm/samspill. Verktøystøtte er
  versjonsømfintlig. Marginal gevinst; ikke verdt en port alene.

### B. Raspberry Pi Zero 2 W (størst kodegjenbruk)
- Full Linux, WiFi + BT, quad-core. **Kan kjøre vår eksisterende
  `elduro-capture` Rust-agent uendret** (samme btleplug/BlueZ som raven) →
  ingen firmware, ingen PMD-reimplementasjon, ekte filsystem (microSD native),
  ekte veggklokke (NTP «gratis»), enkel TLS-WebSocket.
- **Kostnader:** ~1–1,5 W (mot ESP32-ens ~0,3–0,7 W) → 1000 mAh @3,7 V (~3,7 Wh)
  gir ~2,5–3,5 t; lange ritt trenger større batteri. ~10–20 s boot. Større
  (65×30 mm). SD-korrupsjon ved brå strømkutt må håndteres (read-only rootfs +
  tmpfs, eller journalført fs) – vi så nettopp hvor brutalt strømbrudd tester
  systemet. BLE-sentral på Pi er brukbar, men mindre deterministisk enn
  ESP-IDF/Nordic.
- **Leverandørkilder antyder pålitelighetsbekymringer og dårligere
  strømtall** enn embedded – så dette er «reuse everything», ikke «best i
  felt».

### C. nRF5340 + nRF7002 (best RF, høyest innsats)
- Dedikert BLE-SoC (nRF5340) + WiFi 6-companion (nRF7002) over SPI/QSPI.
  **Beste svar på vår coex-smerte:** BLE har egen radio, og en dedikert
  3-tråds coex-linje arbitrerer 2,4 GHz i maskinvare (langt mer robust enn
  ESP32-ens tidsdeling). WiFi kan også kjøres på 5 GHz og unngå 2,4 GHz-
  konflikten helt. Nordics BLE-stack er bransjeledende for sentral-rollen.
- **Nyanse:** de to er fortsatt begge 2,4 GHz (kan dele antenne) og sender
  ikke samtidig – men BLE blir aldri «utsultet» av WiFi slik én-radio-designet
  gjør.
- **Kostnader:** ingen ferdig XIAO-liten modul med begge; krever tilpasset kort
  (Raytac/Fanstel/IndieSemi-moduler finnes, ~17×21 mm) + **Zephyr/nRF Connect
  SDK** (bratt læringskurve) + full firmware-port. Høy innsats, best resultat.

### D. Raspberry Pi Pico 2 W (RP2350 + CYW43439)
- Billig, liten, WiFi + BLE. Men **én delt radio** (samme coex-klasse), og
  BLE-sentral i SDK-en er mindre moden enn ESP-IDF/Nordic. Ingen klar gevinst
  over ESP32-S3.

## Anbefaling (til beslutning – norm «bestilling»)

1. **Behold ESP32-S3 nå.** Den er stabilisert; ikke bytt reaktivt.
2. **Lavrisiko-eksperiment med høy verdi:** siden vi allerede har
   `elduro-capture` i Rust, kjør den på en **Pi Zero 2 W** og mål om den blir
   en bedre feltbro (fjerner coex-klassen, gjenbruker all dekoding/opptak). Én
   ~150-kr-enhet, ingen firmware. Test strøm/varighet og BLE-stabilitet mot
   ESP32-en. Dette er det mest lærerike neste steget hvis vi vil evaluere bytte.
3. **Langsiktig «beste felt-RF»:** hvis coex noen gang blir en blokker igjen,
   er nRF5340+nRF7002 målet – men som et bevisst kort-/Zephyr-prosjekt, ikke en
   rask sving.

Ingenting bestilt – dette er en beslutning for Jørn.
