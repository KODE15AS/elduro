# Elduro frame schema, identity and timestamp model

Status: DRAFT for decision in chat 2. Once agreed, this section is FROZEN and
all agents, ingest paths and the archive must conform. The archive *encoding*
(JSONL vs CBOR vs Parquet vs EDF+) is a separate, later decision; its
requirements are listed at the end.

## 1. Why this document

The field capture unit (XIAO ESP32-S3 Sense bridging the Polar H10 over the
iPhone hotspot) means the same logical data now has:

- two producers: the ESP32 field agent and the raven BT-600 bench agent
  (later: dual H10, phone GPS),
- two delivery modes: live streaming (lossy, whatever the cellular link
  delivered) and batch catch-up (complete, from SD offload or resent queues).

The archive therefore needs a stable *identity* per sample so that batch data
merges idempotently over live data. That identity model, the frame schema and
the clock model are what this document freezes.

## 2. Concepts

- **Sample**: one measurement (one ECG voltage, one ACC xyz triple, one HR
  reading).
- **Frame**: a batch of consecutive samples from one stream as delivered by
  the H10 (ECG frames ~73 samples, ACC frames similar). The frame is the unit
  of transport, storage and dedup.
- **Stream**: one kind of signal from one device: `ecg`, `acc`, `hr`
  (later: `gps`, `imu` from the phone or the XIAO's own sensors).
- **Session**: one continuous recording attempt by one agent. A session may
  reach raven multiple times (live + batch); deliveries of the same session
  merge into one archive session.
- **Agent**: the capturing host: `raven` (BT-600), `xiao-01` (field unit),
  `lenovo` (WinRT).
- **Source**: agent + radio, e.g. `raven:hci1`, `xiao-01:ble`.

## 3. Identity model (the dedup key)

A frame is uniquely identified by:

    (device_id, stream, ts_device_ns)

- `device_id`: stable sensor hardware identity. For the H10 use the device
  MAC (`24:AC:AC:0B:05:2A`), not the advertised name.
- `stream`: as above.
- `ts_device_ns`: the sensor's own timestamp of the frame's first sample,
  in nanoseconds since the device epoch (Polar epoch 2000-01-01T00:00:00Z).

Rationale: the H10's clock is the only clock present in every copy of a
frame regardless of which agent captured it or when it arrived. Two
deliveries of the same frame have identical `ts_device_ns` bit-for-bit,
because it is read from the PMD frame header, not computed.

Merge rule: batch (complete) data wins over live (possibly gappy) data for
the same key. Frames from *different agents* for the same device+stream+time
are kept separately (they are distinct observations; source stays attached).

## 4. Clock domains

Every frame records two clocks; a third arrives with GPS later.

1. `ts_device_ns` - the sensor clock (Polar epoch). Monotonic per device,
   drifts relative to wall time. Identity and intra-device alignment.
2. `ts_host_ns` + `agent` - wall-clock receive time (Unix epoch, ns) on the
   capture host, and whose clock it is. The ESP32 has no RTC: it must sync
   NTP over the hotspot at session start and record boot-offset-corrected
   Unix time; if NTP is unavailable, it records monotonic-since-boot time and
   sets `clock: "unsynced"` so post-processing knows to align via overlap.
3. GPS time (later, phone or L76K module) - bridges everything to true UTC.

Cross-device alignment (dual H10, phone IMU vs strap ACC) is done in
post-processing from these fields; agents never rewrite timestamps.

## 5. Canonical frame schema (logical fields, encoding-agnostic)

Required on every frame:

| Field | Type | Meaning |
|---|---|---|
| `device_id` | string | sensor hardware id (H10 MAC) |
| `stream` | string | `ecg`, `acc`, `hr`, ... |
| `ts_device_ns` | u64 | device clock, first sample of frame |
| `ts_host_ns` | u64 | receive time on capture host, Unix ns |
| `agent` | string | capture host id (`raven`, `xiao-01`, `lenovo`) |
| `seq` | u32 | per-session, per-stream frame counter from the agent |
| `payload` | per stream | see below |

Stream payloads:

- `ecg`: `uv`: array of i32 microvolts (130 Hz, consecutive).
- `acc`: `mg`: array of [x,y,z] i16 milli-g (200 Hz, consecutive).
- `hr`: `bpm`: u16, `rr_ms`: array of u16 (RR intervals since last frame).

Raw bytes (`raw` hex of the PMD frame) are OPTIONAL and recommended for the
raven bench agent (research-grade provenance, decoder re-verification) but
omitted on the ESP32 wire path to save bandwidth; the SD spill MAY keep them.

Required once per session (header record):

| Field | Meaning |
|---|---|
| `session_id` | globally unique: `{agent}-{started_unix_ns}-{4 random hex}` |
| `agent`, `source` | who captured, over which radio |
| `device_id`, `device_name` | the sensor |
| `streams` | per stream: `sample_rate_hz`, `resolution_bits`, `unit`, `range` |
| `device_epoch` | `2000-01-01T00:00:00Z` for Polar |
| `clock` | `ntp-synced` / `unsynced` (ESP32), `host` (raven/lenovo) |
| `schema_version` | 2 (current recorder files are version 1) |

Delivery metadata (`delivery: live` or `batch`) belongs to the transport and
ingest log, not to frame identity.

## 6. The three layers this schema feeds

1. **Wire** (agent -> raven, live): today JSON over WebSocket `/ws/agent`;
   for the ESP32 likely compact JSON or CBOR over MQTT QoS 1. Link-specific,
   may batch multiple frames per message, carries exactly the fields above.
2. **Field spill** (ESP32 SD card): append-only segment files, one directory
   per session, header record first, frames in `seq` order, safe to truncate
   at any byte (crash tolerance). Exact bytes decided with the firmware; must
   convert losslessly to the archive.
3. **Archive** (raven, canonical): encoding TBD - requirements:
   - session-oriented, all frames carry/imply the full identity key,
   - idempotent merge: re-ingesting any delivery is a no-op,
   - acceptance test: ingest live (gappy) then batch (complete) copies of
     the same session; result equals ingesting batch alone,
   - streams separable (ECG-only export without touching ACC),
   - append-friendly during live capture; no rewrite-the-world on each frame.

## 7. Mapping from current v1 recordings

Current `recordings/*.jsonl` (schema_version 1, raven agent only):

| v1 | v2 |
|---|---|
| header `device` (name) | `device_name`; `device_id` must be backfilled from name/MAC table |
| header `source` `raven:hci1` | `agent: raven`, `source: raven:hci1` |
| header `started_unix_ns` | part of `session_id`; kept |
| frame `ts_device_ns`, `ts_host_ns` | unchanged |
| frame `uv` / `raw`, `n` | `payload.uv` / optional `raw`; `n` derivable |
| (missing) | `seq`: assign by file order on migration |

v1 files remain readable; a one-shot migration tool should lift them to v2
when the archive encoding lands.
