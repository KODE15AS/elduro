# Elduro – Handover

Levende spesifikasjon for Elduro (norm «handover» i
[raven-platform](https://github.com/KODE15AS/raven-platform)). Ny chat starter
med den nyeste daterte overgangen (se Historikk nederst) – det er et komplett,
selvstendig dokument – og bruker deretter [README](../README.md) og denne filen
for status/backlog. Punktene krysses av etter hvert som de leveres.

## Status

- [x] **Fase 1 – standard live-strømming:** GATT Heart Rate Profile, live BPM
  og R-R-intervaller.
- [x] **Fase 2 – avansert live-strømming:** rå enkeltavlednings-EKG (130 Hz,
  mikrovolt) og 3-akse akselerometer (200 Hz, milli-g) via Polars PMD-tjeneste,
  live-skop og tapsfri opptak til disk.
- [x] **ESP32-S3 feltbro:** strømmer H10 (EKG + ACC + nativ HR/RR) til
  `wss://elduro.no/ws/agent` over iPhone-hotspot; verifisert ende-til-ende.
- [x] **HRV/RMSSD produksjonssatt:** felles skop-motor
  (`frontend/src/lib/ecgScope.ts`), Rhythm/HRV-visning, nativ RR som primær.
- [x] **Offentlig hosting:** https://elduro.no via Caddy (Let's Encrypt).
- [x] **Frame-skjema frosset:** [docs/format/frame-schema.md](../docs/format/frame-schema.md)
  (schema_version 2). `recordings/*.jsonl` på disk er fortsatt v1 og skal
  migreres.
- [x] **Chat 4 – robust feltbro + dataarkiv (21.09.2026):** microSD
  store-and-forward (etappe 1), MariaDB-arkiv i drift, SNTP-veggklokke,
  auto-håndtering av avtak (hudkontakt-bit), backend-auto-gjenopptak,
  selvhelbredende BLE, samlet TILKOBLING-fane, RAW ACC-fane, HR Compare fjernet.
  **Full detalj, beslutninger, driftsregler og chat-5-backlog i det komplette
  dokumentet [2026-09-21-chat-4-til-5.md](./2026-09-21-chat-4-til-5.md) – start
  der for chat 5.**

## Gjenstående (prioritert backlog – detaljer i 4→5-dokumentet)

- [ ] **SD-spill-opplasting + dedup/merge i backend** (når nytt Sense-kort er
  her; `INSERT IGNORE` på dedup-nøkkelen).
- [ ] **v1-migrering** av `recordings/*.jsonl` (3,9 GB) inn i MariaDB.
- [ ] **Robusthets-finpuss:** full avtak → BLE faller → auto-rekobling kan
  henge ~1 min (STOPP+START gjenoppretter på ~2 s); tydeligere pause-visning.
- [ ] **Raskere hudkontakt-visning** (umiddelbar telemetri ved kontaktendring).
- [ ] **EKG-strimmelens y-skala** finjusteres (ser mindre ut etter høydeendring).
- [ ] **PSRAM-ringbuffer** mellom BLE-inntak og WiFi/SD-skriverne.
- [ ] **Polar H10 nr. 2 (dual-H10):** to broer/sentraler, arbitrering per
  enhet, to rå-strimler + hjerteslag-forankret synk, så syntetisk EKG
  ([docs/architecture/syntetisk-ekg-dual-h10.md](../docs/architecture/syntetisk-ekg-dual-h10.md)).
- [ ] **Batteri/kapsling/effektbudsjett** (Grove Base krever lodding; 1S 1000
  mAh LiPo; 31+ min økt).
- [ ] **Hendelsesmarkør:** ACC-tapp som MVP; eventuelt ESP32-knapp.
- [ ] **Kubios-validering** av RMSSD-tallene (engangs, benk-opptak).
- [ ] **Ingress-migrering Caddy → Traefik** (Jørns beslutning 28.08.2026, se
  [2026-08-06-chat-3-til-4.md](./2026-08-06-chat-3-til-4.md) §8). Koordineres
  med Klasserommet stage-4.
- [ ] **Nytt Sense-kort** (113991115) monteres når det kommer → SD-spill igjen.

Kjent forbehold: H10 trenger ~5–35 s oppvarming før første EKG/HR-ramme etter
start (sensoradferd, ikke bug). H10-en fortsetter å måle/drenere til strømmen
termineres (Polar Issue 2) – derav auto-pause/stopp-logikken. Ekte belte-reset
= ta ut knappcellen. DIS-firmwarestreng ≠ Polar Flow-versjon.

## Vurderinger (chat 4, til beslutning/referanse)

- **Arkivkoding:** [docs/format/arkivkoding-vurdering.md](../docs/format/arkivkoding-vurdering.md)
  (MariaDB, besluttet og i drift).
- **USB-lagring / SSD-powerbank:** [docs/hardware/usb-lagring-vurdering.md](../docs/hardware/usb-lagring-vurdering.md)
  (forkastet for ESP32-formål).
- **Alternative CPU-er:** [docs/hardware/mcu-alternativer-vurdering.md](../docs/hardware/mcu-alternativer-vurdering.md)
  (behold ESP32-S3; Pi Zero 2 W notert for fremtiden).
- **Syntetisk EKG (dual-H10):** [docs/architecture/syntetisk-ekg-dual-h10.md](../docs/architecture/syntetisk-ekg-dual-h10.md).

## Historikk (daterte tillegg, immutable)

| Dato | Overgang | Fil |
|---|---|---|
| 2026-07-29 | chat 1 → 2 | [2026-07-29-chat-1-til-2.md](./2026-07-29-chat-1-til-2.md) |
| 2026-07-29 | chat 2 → 3 | [2026-07-29-chat-2-til-3.md](./2026-07-29-chat-2-til-3.md) |
| 2026-08-06 | chat 3 → 4 | [2026-08-06-chat-3-til-4.md](./2026-08-06-chat-3-til-4.md) |
| 2026-09-21 | chat 4 → 5 | [2026-09-21-chat-4-til-5.md](./2026-09-21-chat-4-til-5.md) |

Konvensjon: aldri revider et avsluttet datert dokument; hver overgang får et
nytt, komplett datert dokument, og status/backlog i denne filen oppdateres.
