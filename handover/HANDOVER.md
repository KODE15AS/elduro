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
- [ ] **Åpent: backend-arbitrering** «nyeste start vinner» per enhet, så to
  faner/kilder ikke kan sloss om beltet (sett i praksis 19.09).
- [ ] Siste hrv-verifisering etter antennesjekk, så vanlig backlog under.

Driftsregler på benk: én fane styrer start/stopp; hotspoten må stå på med
skjermen åpen for ESP32-broen; BT-600 og ESP32 kan nå stå påslått samtidig
(broen holder ikke beltet lenger i idle).

## Gjenstående (prioritert backlog)

- [ ] **microSD store-and-forward (FatFs)** – største gjenstående
  feltpålitelighets-gap; ingen datatap når WiFi faller ut.
- [ ] **Arkiv-koding** – avgjøres sammen med SD-spillformatet (samme problem);
  se frame-schema seksjon 6.
- [ ] **PSRAM-ringbuffer** mellom BLE-inntak og WiFi/SD-skriverne.
- [ ] **Ekte veggklokke på ESP32 (SNTP)** for korpus-justering på tvers av
  økter og enheter.
- [ ] **Felt-/mobil-UI:** styrelayout, wake lock, aggressiv reconnect.
- [ ] **Hendelsesmarkør:** ACC-tapp som MVP; eventuelt ESP32-knapp.
- [ ] **Batteri/kapsling/effektbudsjett** for 31+ min økt (1S 1000 mAh LiPo).
- [ ] **HR Compare med ESP32 som kilde** (`mode: hr`) – verifiser.
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
