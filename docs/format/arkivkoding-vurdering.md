# Arkivkoding – vurdering (chat 4, 20.09.2026)

Forbereder «Archive encoding» (åpen siden chat 2, se
[frame-schema.md](./frame-schema.md) §6). **Beslutning fra Jørn 20.09.2026:
på raven lagrer vi i MariaDB.** Dette dokumentet vurderer datastrømmene og
foreslår hvordan MariaDB-beslutningen realiseres innenfor det frosne
frame-skjemaet (schema_version 2). Selve skjemaet endres ikke.

## 1. Datastrømmene i tall

Målt fra live-driften (ESP32-bro og BT-600, verifisert 20.09):

| Strøm | Rate | Rammer/s | Payload per time (JSON) | Rader per time |
|---|---|---|---|---|
| EKG | 130 Hz, int24 µV | ~1,8 (~73 samples/ramme) | ~4–5 MB | ~6 400 |
| ACC | 200 Hz × 3 akser int16 mg | ~5,5 (~36 tripler/ramme) | ~14–16 MB | ~20 000 |
| HR/RR | ~1 Hz | ~1 | < 0,5 MB | ~3 600 |
| **Sum** | | **~8,3** | **~20 MB/t** | **~30 000/t** |

Empiri: `recordings/` har 3,9 GB v1-JSONL etter benk-perioden; en typisk
kjøretur (1–2 t) gir 20–40 MB. Et 2–3-måneders korpus på 100–150 timer blir
~3–5 millioner rader og 2–3 GB payload – **trivielt for MariaDB 11.4/InnoDB**.
Radraten på ~8 inserts/s live er neglisjerbar.

## 2. Anbefalt arkitektur

Tre lag, som i frame-schema §6, med MariaDB i midten:

1. **Forensisk kaldarkiv (immutabelt, filer):** rå JSONL fra live-opptak og
   SD-spillsegmenter beholdes urørt på disk slik de ble skrevet
   (`recordings/`, SD-kortets `S<boot>-<uptime>/`). Aldri kilde for spørringer,
   alltid kilde for re-ingest.
2. **Kanonisk arkiv (MariaDB, spørrbart):** alle rammer og all metadata i
   normaliserte tabeller med dedup-nøkkelen fra det frosne skjemaet
   `(device_id, stream, ts_device_ns)` som UNIQUE-indeks. Da blir
   idempotent merge (`INSERT IGNORE`) en egenskap av skjemaet, og
   akseptansetesten i §6 (live gappy + batch komplett = batch alene) holder
   ved konstruksjon.
3. **Treningslag (generert):** Parquet/DuckDB-eksport fra MariaDB ved behov;
   aldri håndredigert, alltid reproduserbart.

## 3. Skjemautkast (MariaDB)

```sql
CREATE TABLE sessions (
  session_id   VARCHAR(64) PRIMARY KEY,        -- {agent}-{start_ns}-{hex} (schema §5)
  subject_id   VARCHAR(32) NOT NULL,
  agent        VARCHAR(32) NOT NULL,
  source       VARCHAR(48) NOT NULL,
  device_id    VARCHAR(32) NOT NULL,           -- H10 MAC
  device_name  VARCHAR(64),
  mode         ENUM('ecg','hrv','hr') NOT NULL,
  started_ns   BIGINT UNSIGNED NOT NULL,
  ended_ns     BIGINT UNSIGNED,
  clock        ENUM('ntp-synced','unsynced','host') NOT NULL,
  origin       ENUM('live','spill','migrert-v1') NOT NULL,
  notes        TEXT
);

CREATE TABLE frames (
  device_id    VARCHAR(32)     NOT NULL,
  stream       ENUM('ecg','acc','hr') NOT NULL,
  ts_device_ns BIGINT UNSIGNED NOT NULL,
  session_id   VARCHAR(64)     NOT NULL,
  seq          INT UNSIGNED    NOT NULL,
  ts_host_ns   BIGINT UNSIGNED NOT NULL,
  n_samples    SMALLINT UNSIGNED NOT NULL,
  payload      JSON            NOT NULL,        -- samples-arrayet fra wire-JSON
  PRIMARY KEY (device_id, stream, ts_device_ns), -- dedup-nøkkelen fra skjemaet
  KEY by_session (session_id, stream, seq),
  KEY by_time (device_id, stream, ts_host_ns)
);

CREATE TABLE annotations (                       -- labels er førsteklasses (schema §4)
  id           BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  subject_id   VARCHAR(32) NOT NULL,
  t_start_ns   BIGINT UNSIGNED NOT NULL,
  t_end_ns     BIGINT UNSIGNED NOT NULL,
  label        VARCHAR(48) NOT NULL,
  value        TEXT,
  source       ENUM('self-live','self-review','clinician','algo') NOT NULL,
  confidence   FLOAT,
  entry_latency_ms INT UNSIGNED,
  entry_method VARCHAR(32),
  KEY by_subject_time (subject_id, t_start_ns)
);
```

- **Payload som JSON først** (rett fra wire-formatet, null konvertering,
  lesbart, verktøyvennlig). Volumene (kap. 1) forsvarer ikke kompleksiteten
  ved pakket binær BLOB nå; kan innføres senere som `payload_bin` uten
  skjemabrudd hvis korpuset vokser 100×.
- «All EKG for subjekt X i [t1,t2] med labels» blir én indeksert JOIN –
  det som i §6 var tiltenkt en egen SQLite/DuckDB-indeks dekkes av MariaDB.

## 4. Ingest-veier

1. **Live:** backend (axum) skriver hver ramme til MariaDB med batched
   inserts (f.eks. 1 s buffering); `INSERT IGNORE` gjør reconnect-duplikater
   harmløse.
2. **SD-spill-opplasting (gjenstående firmware-etappe):** segmentene lastes
   opp over WS ved reconnect og går gjennom samme `INSERT IGNORE`-vei –
   dedup/merge kommer gratis.
3. **v1-migrering:** `recordings/*.jsonl` (3,9 GB) mappes per frame-schema §7
   og ingestes med `origin='migrert-v1'`. Filene beholdes som kaldarkiv.

## 5. Avklaringer før implementasjon

1. **Egen MariaDB-container i elduro-stacken** (anbefalt, norm «container»:
   databasen hører til tjenesten) **vs. dele `mariadb:11.4`-containeren som
   allerede kjører for WordPress** (mindre ressursbruk, men kobler
   uavhengige tjenester). Anbefaling: egen `db`-service i elduros
   `docker-compose.yml` med volum på disk, MariaDB 11.4 LTS.
2. Retensjon i `frames` vs. kaldarkivet: behold alt (volumene er små).
3. SNTP på ESP32 (backlog) løfter `clock` fra `unsynced` til `ntp-synced`
   og gjør korpus-tid på tvers av enheter troverdig – bør tas før
   spill-opplastingen implementeres.

## 6. Akseptansetest (fra frame-schema §6)

Ingest en live (hullete) kopi og deretter batch-kopien (komplett) av samme
økt; `SELECT`-resultatet skal være identisk med å ingeste batch alene.
Med UNIQUE-nøkkelen `(device_id, stream, ts_device_ns)` og `INSERT IGNORE`
er dette en skjemaegenskap, men testen skrives likevel som en integrasjonstest
mot en wipbar test-database før produksjonssetting.
