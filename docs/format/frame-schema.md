# Elduro frame schema, identity and timestamp model

Status: agreed in chat 2, pending final read-through, then FROZEN. Includes the
ML-corpus accommodations agreed in chat 2 (subject identity, label stream,
canonical-vs-training tiers). The archive *encoding* (JSONL vs CBOR vs Parquet
vs EDF+) is a separate, later decision; its requirements are in section 6.

## 1. Why this document

The field capture unit (XIAO ESP32-S3 Sense bridging the Polar H10 over the
iPhone hotspot) means the same logical data now has:

- two producers: the ESP32 field agent and the raven BT-600 bench agent
  (later: dual H10, phone GPS),
- two delivery modes: live streaming (lossy, whatever the cellular link
  delivered) and batch catch-up (complete, from SD offload or resent queues).

A later extension collects 2-3 months of recordings plus symptom/condition
metadata for model training. That makes this a longitudinal, *labelled*
dataset, not just a set of sessions. The identity, timestamp, label and
tiering model below are what this document freezes.

## 2. Concepts

- **Sample**: one measurement (one ECG voltage, one ACC xyz triple, one HR
  reading).
- **Frame**: a batch of consecutive samples from one stream as delivered by
  the H10. The frame is the unit of transport, storage and dedup.
- **Stream**: one kind of signal: `ecg`, `acc`, `hr`, `annotation`
  (later: `gps`, `imu`, and derived `beats`).
- **Session**: one continuous recording attempt by one agent. A session may
  reach raven multiple times (live + batch); deliveries merge into one.
- **Agent**: the capturing host: `raven` (BT-600), `xiao-01` (field unit),
  `lenovo` (WinRT).
- **Source**: agent + radio, e.g. `raven:hci1`, `xiao-01:ble`.
- **Subject**: the human the signals belong to. Sits ABOVE device: dual-H10
  or a swapped strap still map to one `subject_id`. ML grouping/splitting is
  by subject.

## 3. Identity model (the dedup key)

A signal frame is uniquely identified by:

    (device_id, stream, ts_device_ns)

- `device_id`: stable sensor hardware identity (H10 MAC `24:AC:AC:0B:05:2A`),
  not the advertised name.
- `stream`, `ts_device_ns`: stream type and the sensor's own timestamp of the
  frame's first sample (Polar epoch 2000-01-01T00:00:00Z).

Rationale: the H10 clock is the only clock present in every copy of a frame
regardless of capturing agent or arrival time. Two deliveries of the same
frame have identical `ts_device_ns` bit-for-bit (read from the PMD header,
not computed).

Merge rule: batch (complete) data wins over live (gappy) data for the same
key. Frames from different agents for the same device+stream+time are kept
separately (distinct observations; source stays attached).

## 4. Clock domains

Every frame records two clocks; a third arrives with GPS later.

1. `ts_device_ns` - sensor clock (Polar epoch). Identity and intra-device
   alignment.
2. `ts_host_ns` + `agent` - wall-clock receive time (Unix ns) on the capture
   host, and whose clock it is. The ESP32 has no RTC: sync NTP over the
   hotspot at session start; if unavailable, record monotonic-since-boot time
   and set `clock: "unsynced"` so post-processing aligns via overlap.
3. GPS time (later) - bridges everything to true UTC.

Agents never rewrite timestamps; cross-device alignment is done in
post-processing.

## 5. Canonical schema (logical fields, encoding-agnostic)

### 5.1 Signal frame (ecg / acc / hr)

| Field | Type | Meaning |
|---|---|---|
| `subject_id` | string | human the data belongs to |
| `device_id` | string | sensor hardware id (H10 MAC) |
| `stream` | string | `ecg`, `acc`, `hr`, ... |
| `ts_device_ns` | u64 | device clock, first sample of frame |
| `ts_host_ns` | u64 | receive time on capture host, Unix ns |
| `agent` | string | capture host id |
| `seq` | u32 | per-session, per-stream frame counter |
| `payload` | per stream | see below |

Stream payloads:
- `ecg`: `uv`: array of i32 microvolts (130 Hz, consecutive).
- `acc`: `mg`: array of [x,y,z] i16 milli-g (200 Hz, consecutive).
- `hr`: `bpm`: u16, `rr_ms`: array of u16 (RR intervals since last frame).

Raw PMD bytes (`raw` hex) are OPTIONAL, recommended on the raven bench agent
for provenance, omitted on the ESP32 wire path to save bandwidth.

### 5.2 Annotation / label frame (symptoms, markers, conditions)

Labels are a FIRST-CLASS time-indexed stream, never embedded in signal
frames, so labels can be corrected/relabelled without touching raw signal.

| Field | Type | Meaning |
|---|---|---|
| `subject_id` | string | who |
| `stream` | string | `annotation` |
| `t_start_ns`, `t_end_ns` | u64 | interval on the same clock as signals (point event: equal) |
| `label` | string | e.g. `palpitation`, `dizziness`, `chest`, `marker` |
| `value` | optional | free text / structured detail |
| `source` | string | who/what produced it: `self-live`, `self-review`, `clinician`, `algo` |
| `confidence` | optional f32 | 0-1 |
| `entry_latency_ms` | optional u32 | delay between event and logging (see below) |
| `entry_method` | string | `esp32-button`, `strap-tap`, `web-ui`, ... |

Rules:
- Multi-source labels for the same time range are KEPT SEPARATE (a live
  self-report and a later clinician review are distinct observations).
- `entry_latency_ms` matters: a symptom button pressed while riding lags the
  real ECG event; training should widen label windows accordingly. The
  validated finger-tap-on-strap ACC spike is a near-zero-latency marker and a
  good MVP annotation channel.

### 5.3 Session header (once per session)

| Field | Meaning |
|---|---|
| `session_id` | `{agent}-{started_unix_ns}-{4 random hex}` |
| `subject_id`, `agent`, `source` | who/what/where |
| `device_id`, `device_name` | the sensor |
| `streams` | per stream: `sample_rate_hz`, `resolution_bits`, `unit`, `range` |
| `device_epoch` | `2000-01-01T00:00:00Z` for Polar |
| `clock` | `ntp-synced` / `unsynced` / `host` |
| `schema_version` | 2 (current recorder files are version 1) |

Delivery metadata (`delivery: live|batch`) belongs to transport/ingest logs,
not to frame identity.

## 6. Two-tier storage + the three transport layers

Corpus principle: the raw archive is IMMUTABLE and append-only. The training
set is REGENERATED from it, never hand-edited, so it is always reproducible.

- **Canonical archive (raven, immutable):** session-oriented; every frame
  carries/implies the full identity key; idempotent merge (re-ingesting any
  delivery is a no-op); streams separable; append-friendly during live
  capture. Acceptance test: ingest live (gappy) then batch (complete) copies
  of a session; result equals ingesting batch alone.
- **Training layer (raven, derived):** windowed, R-peak-annotated, resampled,
  feature-ready (candidate: Parquet queried with DuckDB); rebuilt from
  canonical + current labels on demand.
- **Index (raven):** SQLite/DuckDB mapping subject/stream/time-range ->
  file+offset, plus the labels table, so "all ECG for subject X in [t1,t2]
  with labels" is cheap across sessions.

Transport layers (distinct from storage):
1. **Wire** (agent->raven, live): JSON over WS today; likely CBOR over MQTT
   QoS 1 for the ESP32. May batch multiple frames per message.
2. **Field spill** (ESP32 SD): append-only segment files, one dir per
   session, header first, frames in `seq` order, safe to truncate at any byte.
   Exact bytes decided with the firmware; must convert losslessly to canonical.
3. **Archive**: as above.

## 7. Mapping from current v1 recordings

| v1 | v2 |
|---|---|
| header `device` (name) | `device_name`; backfill `device_id` from name/MAC table |
| header `source` `raven:hci1` | `agent: raven`, `source: raven:hci1` |
| header `started_unix_ns` | part of `session_id`; kept |
| frame `ts_device_ns`, `ts_host_ns` | unchanged |
| frame `uv`/`raw`, `n` | `payload.uv` / optional `raw`; `n` derivable |
| (missing) | `subject_id` default to the single current subject; `seq` by file order |

v1 files stay readable; a one-shot migration lifts them to v2 when the
archive encoding lands.
