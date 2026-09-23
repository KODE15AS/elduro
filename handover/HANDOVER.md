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
  Detaljer i [2026-09-21-chat-4-til-5.md](./2026-09-21-chat-4-til-5.md).
- [x] **Chat 5 – dual-H10 benk + syntetisk EKG v1 + UI-finpuss (23.09.2026):**
  begge belter samtidig (arbitrering per belte, beltefilter i agent),
  syntesemotor i backend (R-topp-klokkefit + fusjon, replay-validert:
  100 % match, ~3,2 ms residual), SYNTETISK EKG-fane, RAW-faner uten
  kildevelger med stablet A-øverst, belteregister (ELDURO_BELTS),
  hudkontakt-telemetri fra benkeagenten. **Full detalj, driftsregler og
  chat-6-backlog i det komplette dokumentet
  [2026-09-23-chat-5-til-6.md](./2026-09-23-chat-5-til-6.md) – start der for
  chat 6.**

## Gjenstående (prioritert backlog – detaljer i 4→5-dokumentet)

Prioritert 23.09.2026 (chat 5→6); full detalj i
[2026-09-23-chat-5-til-6.md](./2026-09-23-chat-5-til-6.md) §3.

- [ ] **ESP32-firmware: beltefilter** (kun belte A, konfigurerbart) +
  umiddelbar hudkontakt-telemetri. Krever USB-flash. Workaround til da:
  start B (BT-600) før A (ESP32) ved scratch-oppstart.
- [ ] **Konsensus-HRV/RMSSD** (slag begge belter ser; ACC/SQI-vekting) -
  IKKE deriver RR fra den syntetiske strimmelen.
- [ ] **Plan vektorsløyfe (2D-VCG)** + fusjonsmetode 2/3 (kvalitetsvekting).
- [ ] **Valider syntesen mot 12-avlednings-referansen** (negativ T V2-V6,
  PQ 237 ms gjenkjennbart; `docs/private/`, gitignorert).
- [ ] **frames.device_id = belte-ID** (i dag = kilde) + belteplassering som
  øktmetadata; må inn før SD-opplastingens dedup.
- [ ] **SD-spill-opplasting** (venter nytt Sense-kort 113991115) og
  **v1-migrering** av `recordings/*.jsonl` (3,9 GB).
- [ ] **Robusthets-finpuss:** auto-rekobling etter full avtak (~1 min heng);
  auto-pause ved avtak i benkeagenten; EKG y-skala; PSRAM-ringbuffer;
  UI-WS-melding med ugyldig kontrolltegn (ettergås).
- [ ] **Dual-H10 felt** (to belter på én ESP32-bro): venter på Sense-kortet.
- [ ] **Batteri/kapsling**, **hendelsesmarkør** (ACC-tapp MVP),
  **Kubios-validering**, **Caddy → Traefik** (koordineres med Klasserommet
  stage-4).

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
| 2026-09-23 | chat 5 → 6 | [2026-09-23-chat-5-til-6.md](./2026-09-23-chat-5-til-6.md) |

Konvensjon: aldri revider et avsluttet datert dokument; hver overgang får et
nytt, komplett datert dokument, og status/backlog i denne filen oppdateres.
