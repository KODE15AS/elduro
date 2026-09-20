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

### Maskinvarehendelse 20.09.2026: Sense-hovedkortet termisk defekt

Ved Grove Base-forberedelsene ble XIAO ESP32-S3 **Sense**-hovedkortet målt til
98–109 °C chip-temperatur (maks-spek 105 °C) i nær tomgang. Eliminasjonstest
frikjente microSD-kort, datterkort og antenne; **reservekortet (XIAO ESP32-S3
plain) måler 44 °C med identisk firmware og antenne** – hovedkortet er defekt
og pensjonert. Mulig bidrag: ukene med antenneløs drift (PA-mismatch).

- **Reservekortet er nå feltbroen** (ny MAC → ny agent-id), men uten
  Sense-datterkort: **ingen SD-spill før nytt Sense-kort er kjøpt**
  (113991115, ~$14 – bestillingspunkt). Datterkortet med SD-slot antas friskt.
- Firmwaren fikk termikk-instrumentering i samme økt: chip-temperatur +
  fritt minne logges hvert minutt, og WiFi-reconnect har nå backoff (1→10 s)
  så en borte hotspot ikke varmer radioen unødig.
- Grove Base krever lodding av batterikontakt – utsatt til loddeøkt.

## Gjenstående (prioritert backlog)

- [~] **microSD store-and-forward (FatFs)** – *etappe 1 levert og
  FELTVERIFISERT 20.09.2026:* SD-spill i firmwaren (FAT32-montering med
  selvtest ved boot, øktkataloger `S<boot>-<uptime>/` med header +
  append-only `frames.jsonl`, `seq` per strøm, fsync hvert 50. frame,
  segment-lukking ved linktap, øktlisting ved boot). Reell test med
  bevegelse (romaskin/gange/hopp, 4–5 m avstand): 4,2 MB frames.jsonl på
  kortet, parallelt med tapsfri live-strøm (130,11 Hz, 0 ts-hull). **Merk:
  GPIO21 deles mellom SD-CS og statuslampen – lampen er deaktivert når kort
  står i (flimrer med SD-aktivitet).** Gjenstår: opplasting/gjenopptak ved
  reconnect og dedup/merge i backend (avhenger av MariaDB-ingesten under).
- Signalkvalitet vurdert 20.09 (fangst + analyse mot BT-600-referanse):
  ESP32-stien er statistisk identisk med benkreferansen i ro og i bevegelse;
  bevegelsesartefakter er elektrodefysikk, ikke kjedefeil. Elektrodene
  trenger noen minutter på kroppen før impedansen setter seg.
- [~] **Arkiv-koding** – *besluttet 20.09.2026 (Jørn): MariaDB i den DELTE
  `mariadb:11.4`-containeren på raven* (dokumentert unntak fra
  container-normen, se AGENTS.md; begrunnelse: felles backup-regime).
  Vurdering: [docs/format/arkivkoding-vurdering.md](../docs/format/arkivkoding-vurdering.md).
  *Implementert 20.09:* skjema i [db/schema.sql](../db/schema.sql) og
  backend-ingest (`backend/src/db.rs`, INSERT IGNORE på dedup-nøkkelen,
  øktbokføring fra kommandostrømmen; aktiveres av `ELDURO_DB_URL` i `.env`,
  passiv uten). *Provisjonert og I DRIFT 20.09:* `elduro`-db + bruker i den
  delte containeren, web joinet `mariadb-nett`, creds i gitignorert `.env`.
  **Akseptansetesten (vurderingen kap. 6) bestått:** 665 rammer levert
  hullete + komplett re-levering ga null duplikater og eksakt samplesum.
  Gjenstår: v1-migrering av `recordings/` (3,9 GB) og SD-spill-opplasting.
  Kjent v1-forbehold: device_id = source til belteidentitet følger rammene.
- [ ] **PSRAM-ringbuffer** mellom BLE-inntak og WiFi/SD-skriverne.
- [ ] **Ekte veggklokke på ESP32 (SNTP)** for korpus-justering på tvers av
  økter og enheter.
- [x] **TILKOBLING-fane (20.09.2026):** all øktstyring flyttet fra
  visningsfanene til én tilkoblingsfane (`/tilkobling`): kildekort per kilde
  (N-kilder-design, klart for dual-H10), modusvalg + start/stopp,
  strømbekreftelse med målte rater mot nominelt (EKG/ACC/HR), Polar-info
  (navn, batteri, BLE-RSSI) og bro-telemetri fra firmware hvert 5. s
  (WiFi-RSSI, chip-temp, heap, SD-status, oppetid). RAW ECG og RHYTHM/HRV er
  rene visninger med kildefilter og pause. Verifisert i produksjon uten
  belte; full E2E-sjekk med belte gjenstår (batteri/BLE-RSSI/rate-dots).
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

Robusthetsfiks 20.09 (strømbrudd-test): brå frakobling (USB-C trukket midt i
økt) ga to separate feil ved rekobling:
1. NimBLE tom for GATT-prosedyrer → «GATTC proc alloc failed» på attr 0x0033.
   Fikset: `CONFIG_BT_NIMBLE_GATT_MAX_PROCS` 4→16 + PMD-vaktbikkje (river ned
   linken for ren rekobling hvis EKG uteblir 40 s etter meldt strømming).
2. Beltet terminerte selv tilkoblingen (`reason=531`) ~0,2–0,8 s etter connect,
   midt i PMD-abonnering (ACC-skriv status 7 = not-connected, hr_val=0x0000) –
   en spøkelsestilkobling på H10 etter det brå bruddet, forverret av 18 %
   batteri. Firmwaren hamret rekobling hvert 300 ms og hindret beltet i å
   rydde. Fikset: progressiv backoff (opp mot 6 s) ved korte/avviste
   tilkoblinger, teller nullstilles først når EKG faktisk flyter, og tydelig
   UI-beskjed etter 4 forsøk om å ta sensoren av stroppen i 30 s.
   **Fysisk kur (belte-side, kan ikke fikses i firmware): strømsykle beltet –
   sensor av stroppen 30 s – for å tømme spøkelsestilkoblingen. Lad/bytt
   batteri (18 %) før feltbruk.**
3. Backend-auto-gjenopptak: `wanted`-kart (source→mode) settes ved start,
   fjernes ved stopp/arbitrering; backend gjensender start når en agent
   (re)registrerer, så ESP32-broen gjenopptar strømming av seg selv etter
   reboot/strømbrudd uten at bruker må trykke START. Deployet, men ennå ikke
   feltverifisert (belte-problemet nedenfor kom i veien for en ren hot-swap).

Åpent belte-problem 20.09 (ikke løst): etter mye testing gikk H10 nr. 1 fra
3+ min stabil strømming til å terminere hver tilkobling (`reason=531`) ~1 s
etter «armed», med batteri som svinger 90/18/−1 %. Mistanke: knappcelle
(CR2025) brownouter under PMD-last, eller fastlåst belte. Neste økt: bytt
CR2025 og/eller test H10 nr. 2 for å isolere belte vs. bro. Firmwaren
håndterer det pent nå (backoff + tydelig UI-beskjed), men kan ikke fikse en
belte som browner ut.

## Historikk (daterte tillegg, immutable)

| Dato | Overgang | Fil |
|---|---|---|
| 2026-07-29 | chat 1 → 2 | [2026-07-29-chat-1-til-2.md](./2026-07-29-chat-1-til-2.md) |
| 2026-07-29 | chat 2 → 3 | [2026-07-29-chat-2-til-3.md](./2026-07-29-chat-2-til-3.md) |
| 2026-08-06 | chat 3 → 4 | [2026-08-06-chat-3-til-4.md](./2026-08-06-chat-3-til-4.md) |

Konvensjon: aldri revider et avsluttet tillegg; legg til et nytt datert
dokument og oppdater status og backlog i denne filen.
