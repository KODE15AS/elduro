# Elduro – Handover

Levende spesifikasjon for Elduro (norm «handover» i
[raven-platform](https://github.com/KODE15AS/raven-platform)). Ny chat starter
her, deretter [README](../README.md) for å navigere prosjektet. Punktene under
krysses av etter hvert som de leveres; chat-overganger arkiveres som daterte
tillegg i denne katalogen (se Historikk nederst).

## Status

- [x] **Fase 1 – standard live-strømming:** GATT Heart Rate Profile, live BPM
  og R-R-intervaller.
- [x] **Fase 2 – avansert live-strømming:** rå enkeltavlednings-EKG (130 Hz,
  mikrovolt) og 3-akse akselerometer (200 Hz, milli-g) via Polars PMD-tjeneste,
  live-skop og tapsfri opptak til disk.
- [x] **ESP32-S3 feltbro (bring-up):** XIAO ESP32-S3 Sense strømmer H10
  (EKG + ACC + nativ HR/RR) til `wss://elduro.no/ws/agent` over
  iPhone-hotspot; hele kjeden er verifisert ende-til-ende.
- [x] **HRV/RMSSD produksjonssatt:** felles skop-motor
  (`frontend/src/lib/ecgScope.ts`), Rhythm/HRV-visning, nativ RR som primær.
- [x] **Offentlig hosting:** https://elduro.no via Caddy (Let's Encrypt).
- [x] **Frame-skjema frosset:** [docs/format/frame-schema.md](../docs/format/frame-schema.md)
  (schema_version 2). `recordings/*.jsonl` på disk er fortsatt v1 og skal
  migreres.

## Pågående (chat 4): feltbro-stabilisering

Status 19.09.2026 (benk-økt, se firmware-commit for detaljer):

- [x] **Firmware stabilisert og verifisert** (EKG 131,6 / ACC 203,2 samples/s
  uten linktap): slipper beltet i idle (ingen kamp med BT-600), RSSI-terskel
  −85 dBm med backoff og live status til UI-et, coex-preferanse BLE under
  tilkobling, 6 s supervision timeout, PMD-start som GATT-kjede med
  kvittering/retry (fikser «streaming uten data»), diagnostikk på notify/drop.
- [x] Begge stier re-verifisert som i chat 3: BT-600 (benk) og ESP32-bro,
  inkl. nativ HR/RR i hrv-modus. H10-batteri på gammelt belte målt til 30 % –
  byttet til nytt belte («Polar H10 1DA2053E», navnesøket fungerer på tvers).
- [x] **Radiomiljø LØST: U.FL-antennen var ikke montert.** Den betjener BÅDE
  WiFi og BLE (én delt 2,4 GHz-radio). Montert 19.09 kveld: RSSI gikk fra
  −80 til −42 dBm, tilkobling på første forsøk. Hele dagens 0x3E-/timeout-
  mysterium var i praksis antenneløs drift.
- [ ] **Siste verifisering gjenstår:** etter antennemontering avsluttet H10-en
  tilkoblingene selv (reason=531 «remote user terminated») – beltet var
  trolig av kroppen/tørt (H10 krever hudkontakt). Neste økt: belte på med
  fuktede elektroder, start hrv fra UI-et, forvent stabil strøm. Merk også
  hr_val=0x0000 i samme runde (HR-discovery racet mot 531-frakoblingene) –
  verifiser at HR/RR kommer når linken står.
- [x] **Backend-arbitrering** «nyeste start vinner»: en start stopper alle
  andre kilder (b2e66bf); verifisert begge veier 20.09. Gjøres per enhet når
  dual-H10 kommer.
- [x] **Sluttverifisering 20.09:** brukerstyrt RECORD fra UI-et ga EKG ~132,
  ACC ~203 samples/s og HR 1,0/s med RR, uten linktap. Viktige tillegg på
  veien: BLE-radioprioritet under strømming (63c9e73, H10 la på ved
  notifikasjonsstall), UI adopterer eksternt startede økter (72db2ff), og
  belte-kur ved 531-frakoblinger: knepp sensoren av stroppen 30 s.
  Merk: beltets «2 Bluetooth-enheter»-innstilling står PÅ (Polar Flow);
  vurder å slå den av hvis tilkoblingsgrums gjenoppstår.

Driftsregler på benk: én fane styrer start/stopp; hotspoten må stå på med
skjermen åpen for ESP32-broen; BT-600 og ESP32 kan nå stå påslått samtidig
(broen holder ikke beltet lenger i idle).

## Gjenstående (prioritert backlog)

- [~] **microSD store-and-forward (FatFs)** – *etappe 1 levert 20.09.2026:*
  SD-spill i firmwaren (FAT32-montering med selvtest ved boot, øktkataloger
  `S<boot>-<uptime>/` med header + append-only `frames.jsonl`, `seq` per
  strøm, fsync hvert 50. frame, segment-lukking ved linktap). Verifisert på
  benk: kort montert (30 GB), selvtest OK. **Merk: GPIO21 deles mellom SD-CS
  og statuslampen – lampen er deaktivert når kort står i.** Gjenstår: full
  øktverifisering med belte, opplasting/gjenopptak ved reconnect og
  dedup/merge i backend (henger sammen med arkivkodingen under).
- [~] **Arkiv-koding** – *beslutning 20.09.2026 (Jørn): lagring i MariaDB på
  raven.* Vurdering med skjemautkast, ingest-veier og akseptansetest skrevet:
  [docs/format/arkivkoding-vurdering.md](../docs/format/arkivkoding-vurdering.md).
  Gjenstår: avklare egen db-container vs. delt (anbefalt: egen), implementere
  backend-ingest og v1-migrering. SD-spillformatet (etappe 1 over) er
  JSONL-kompatibelt med dette by design.
- [ ] **PSRAM-ringbuffer** mellom BLE-inntak og WiFi/SD-skriverne.
- [ ] **Ekte veggklokke på ESP32 (SNTP)** for korpus-justering på tvers av
  økter og enheter.
- [ ] **Felt-/mobil-UI:** styrelayout, wake lock, aggressiv reconnect.
- [ ] **Hendelsesmarkør:** ACC-tapp som MVP; eventuelt ESP32-knapp.
- [ ] **Batteri/kapsling/effektbudsjett** for 31+ min økt (1S 1000 mAh LiPo).
- [x] ~~HR Compare med ESP32 som kilde~~ – **HR Compare-fanen ble fjernet
  20.09.2026** (chat-1-verktøy for radiosammenligning, utspilt; hadde
  60 s-fellen og misvisende «Lenovo»-lane). `mode: hr` består i firmware og
  agent for fremtidig bruk.
- [ ] **Polar H10 nr. 2:** dobbelbelte-plassering, hjerteslag-forankret synk,
  deretter adaptiv LMS-opprydding og AV-blokk-klassifisering.
- [ ] **Kubios-validering** av RMSSD-tallene (engangs, benk-opptak).
- [ ] **Ingress-migrering Caddy → Traefik** (Jørns beslutning 28.08.2026, se
  [2026-08-06-chat-3-til-4.md](./2026-08-06-chat-3-til-4.md) seksjon 8).
  Koordineres med Klasserommet stage-4.

Kjent forbehold: H10 trenger ~5–35 s oppvarming før første EKG/HR-ramme etter
start – sensoradferd, ikke en bug.

## Historikk (daterte tillegg, immutable)

| Dato | Overgang | Fil |
|---|---|---|
| 2026-07-29 | chat 1 → 2 | [2026-07-29-chat-1-til-2.md](./2026-07-29-chat-1-til-2.md) |
| 2026-07-29 | chat 2 → 3 | [2026-07-29-chat-2-til-3.md](./2026-07-29-chat-2-til-3.md) |
| 2026-08-06 | chat 3 → 4 | [2026-08-06-chat-3-til-4.md](./2026-08-06-chat-3-til-4.md) |

Konvensjon: aldri revider et avsluttet tillegg; legg til et nytt datert
dokument og oppdater status og backlog i denne filen.
