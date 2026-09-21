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

Rekkefølge justert 21.09.2026 (chat 5): SD-opplasting venter på maskinvare, så
UI-finpuss og dual-H10-benkarbeid går først.

- [x] **UI-finpuss elduro.no (LEVERT 21.09 natt, Jørns finpuss-dokument):**
  RAW ECG/ACC uten kildevelger - viser automatisk aktive strømmer, to stablede
  strimler/paneler ved dual (A øverst, fast). ACC komprimert til
  EKG-strimmelhøyde. RHYTHM/HRV omdøpt til **SYNTETISK EKG** (path
  `/syntetisk-ekg`, gammel path er alias); all tilkoblingsinfo kun på
  TILKOBLING. Gjenstår fra gammel liste: EKG y-skala-finjustering.
- [x] **Dual-H10 benkstart + syntetisk EKG v1 (LEVERT 21.09):** begge belter
  strømmer samtidig til arkivet (A på ESP32, B på BT-600 med `--device`-lås;
  `ELDURO_DEVICE_PINS` i `.env`). **Syntesemotor i backend**
  (`backend/src/synth.rs`): R-topp-forankret klokke-fit (offset+drift) +
  polaritetsjustert fusjon, kringkastes som kilde `synth` (genereres kun,
  arkiveres ikke). Replay-validert mot kveldens dual-data
  (`--bin synth_replay`): 100 % R-topp-match, ~3,2 ms residual,
  lead-korrelasjon +0,85-0,94. Visning med grunnlag/konfidens og «washed
  out» høyrekant.
- [ ] **Dual-H10 neste trinn:** konsensus-basert HRV/RMSSD (slag begge belter
  ser, ACC/SQI-vekting), plan vektorsløyfe (2D-VCG), metode 2/3-fusjon,
  validering mot 12-avlednings-referansen (`docs/private/`, gitignorert).
  Lærdommer fra benkstarten: (1) drept sentral gir foreldreløs PMD-strøm i
  beltet → agenten fikk stopp-før-start; (2) belte-reset (knappcelle) sletter
  beltets bindingsnøkler → slett BlueZ-bindingen og par på nytt manuelt
  (`bluetoothctl pair` – btleplug har ingen paringsagent); (3) PMD-kontroll
  krever kryptert link, feiler som «Not paired»/«Not connected» uten bond.
- [ ] **frames.device_id = belte-ID, ikke kilde:** ingest bruker i dag source
  som device_id (rammene på wire mangler belte-id). Må fikses før
  SD-opplastingens dedup (samme belte via to stier skal dedupe på belte).
- [ ] **Småfunn 21.09:** UI-hjelpeteksten «Nyeste start vinner …» er utdatert
  etter per-enhet-arbitrering (tas i UI-finpussen); én UI-WS-melding observert
  med ugyldig kontrolltegn i JSON (ettergås).
- [ ] **SD-spill-opplasting + dedup/merge i backend** (når nytt Sense-kort er
  her; `INSERT IGNORE` på dedup-nøkkelen).
- [ ] **v1-migrering** av `recordings/*.jsonl` (3,9 GB) inn i MariaDB.
- [ ] **Robusthets-finpuss:** full avtak → BLE faller → auto-rekobling kan
  henge ~1 min (STOPP+START gjenoppretter på ~2 s); tydeligere pause-visning.
- [ ] **Raskere hudkontakt-visning:** benkeagenten LEVERT 21.09 (telemetri
  umiddelbart ved kontaktendring); ESP32-firmware gjenstår (5 s-kadens).
  Auto-pause ved avtak mangler også i benkeagenten (paritet med firmware).
- [ ] **EKG-strimmelens y-skala** finjusteres (ser mindre ut etter
  høydeendring).
- [ ] **PSRAM-ringbuffer** mellom BLE-inntak og WiFi/SD-skriverne.
- [ ] **Dual-H10 felt** (to belter på én ESP32-bro): venter på nytt Sense-kort;
  benkarbeidet over går først.
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
