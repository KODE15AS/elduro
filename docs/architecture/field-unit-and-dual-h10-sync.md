# Field capture unit and dual-H10 synchronization (build plan)

Status: design. This is the forward plan for the ESP32 datastream and the
second Polar H10. It builds directly on
[../format/frame-schema.md](../format/frame-schema.md) (identity, clocks,
tiers) and the hardware in [../hardware/komponentliste.md](../hardware/komponentliste.md).
It records the decision to do cross-belt synchronization in software on the
shared heartbeat rather than with hardware BLE timing (see section 5).

## 1. What we are building next

The bench pipeline (raven ASUS BT-600 -> WebSocket -> web UI) already proves
live ECG/ACC/HR capture and the clinical scope / HRV views. The next stage
makes the capture wearable and, after that, adds a second belt:

- Stage 1 - single H10 field unit: a XIAO ESP32-S3 Sense worn on the chest,
  BLE central to one Polar H10, logging losslessly to microSD and streaming
  to raven / elduro.no over the iPhone Personal Hotspot.
- Stage 2 - dual H10: a second belt for two near-orthogonal leads, for much
  better P-wave visibility and AV-block classification, with the two streams
  aligned on one timeline.

We do Stage 1 fully first, but design every invariant so Stage 2 drops in
without rework.

## 2. Foundation we lock now (from the frame schema)

These are cheap to honour now and expensive to retrofit, so they are fixed
before any firmware is written. All are already in the frame schema:

- Identity `(device_id, stream, ts_device_ns)`. `device_id` is the H10 MAC;
  `ts_device_ns` is the sensor's own clock (Polar epoch), read from the PMD
  header, never computed.
- Two clocks per frame: `ts_device_ns` (sensor) and `ts_host_ns` + `agent`
  (receive time). The ESP32 has no RTC: NTP over the hotspot at session
  start, else record monotonic-since-boot and mark `clock: "unsynced"` so
  post-processing aligns via overlap.
- `seq`: per-session, per-stream frame counter, so a dropped WiFi packet is
  detectable as a gap rather than a silent hole.
- Lossless raw samples: no resampling in the archive (130 Hz ECG, 200 Hz
  ACC kept raw).
- One `subject_id` above the devices: dual belts or a swapped strap still map
  to one subject.

Given these, the archive *encoding* (JSONL vs CBOR vs Parquet vs EDF+) stays
deferred, exactly as the frame schema says. We choose the container once the
ESP32 wire format and the real bandwidth / gap behaviour are observed.

## 3. Stage 1 - single H10 field unit

Hardware: XIAO ESP32-S3 Sense + external U.FL FPC antenna + Grove Base with a
LiPo + microSD. The camera is never initialized (power); the SD slot lives on
the same detachable board, so that board stays attached.

Firmware (ESP-IDF, NimBLE central):
- Scan and connect to the H10 (filter on the name "Polar H10 ..."; do not rely
  on the PMD service UUID being in the advertisement).
- Enable HR (RR) and the PMD stream (ECG 130 Hz, ACC 200 Hz); parse the PMD
  per-frame device timestamp.
- Build frames per the schema (device_id, stream, ts_device_ns, ts_host_ns,
  agent = `xiao-01`, seq, payload). Omit the raw PMD hex on the wire to save
  bandwidth; keep it on the raven bench agent for provenance.
- Write every frame losslessly to microSD first (store-and-forward), then
  stream to the backend. On reconnect, resend the SD backlog so a cellular
  gap never loses data; the backend dedups on the identity key and prefers
  complete batch data over gappy live data.

Transport: ESP32 -> iPhone Personal Hotspot (2.4 GHz, "Maximize Compatibility"
on) -> raven / elduro.no, WebSocket to `/ws/agent` (same ingestion path the
bench agent uses).

Backend: reuse the existing agent ingestion and the live views. Canonical
recording stays JSONL for now.

## 4. Stage 2 - dual H10 and synchronization

Placement (user's plan): two H10 stacked near normal position, the upper belt
rotated to just below the right nipple and the lower belt to just below the
left nipple. This gives two roughly orthogonal projections of the same heart,
which is what improves P-wave detection and AV-block work.

Identity: one `subject_id`, two `device_id` (two MACs), per the schema. The
ESP32 runs two NimBLE client connections plus the WiFi uplink.

Synchronization approach - heartbeat-anchored, solved in post-processing:

- Each belt already carries its own high-resolution device clock, so timing
  *within* a belt is authoritative and jitter-free.
- The only cross-belt problem is relating belt A's clock to belt B's clock.
  Both belts observe the *same* physical event stream: the heartbeat. Detect
  R-peaks in both ECGs and use them as a continuous shared reference; fit the
  relative clock offset and drift with a linear regression over the session
  (offset = intercept, crystal drift = slope). This is the reference-broadcast
  principle: the shared event cancels the unknown air-time and delivery jitter.
- Coarse initial anchor (optional): a synchronized finger-tap on both straps
  produces a near-simultaneous ACC spike - already a schema-blessed
  near-zero-latency marker - useful to bootstrap the fit and as a UI check.
- Nuance: two leads see the R-peak with a small, roughly *constant*
  physiological inter-lead delay (a few ms). That is separable from crystal
  drift (constant intercept vs. time-varying slope), and for AV-block the key
  measurement (the PR interval) is made *within* a belt anyway.
- Precision: a few ms is enough (and is the ceiling regardless, since each ECG
  sample is 1/130 s = 7.69 ms wide). PR intervals live at tens to hundreds of
  ms, so a few ms alignment is clinically sufficient.

Where it runs: on the backend / offline, not on the microcontroller. The
ESP32 only needs a coarse arrival timestamp (`ts_host_ns`) and to forward both
streams with their device clocks and `seq`; the alignment math happens where
it is easy to test.

Coexistence: two BLE connections plus WiFi share one 2.4 GHz radio, so we lean
on microSD store-and-forward and a modest WiFi duty cycle. Because sync is
post-processed on the device clocks, it does not depend on real-time delivery
at all.

## 5. Rejected alternative: hardware-timed BLE sync ("ZipSync")

An earlier proposal ("ZipSync") aimed for sub-0.5 ms cross-belt sync by
capturing BLE connection-event anchor points in a hardware ISR and phasing the
two connections a half connection-interval apart. The goal is right; the
mechanism is not feasible on this hardware and is not needed:

- ESP-IDF's NimBLE exposes no controller-level anchor-point timestamps to the
  application. The ESP32-S3 BLE controller is a closed blob; the earliest you
  can stamp is the host callback, with ~100 us to ~1 ms of variable delay -
  not sub-100 us. (Nordic's nRF52 Timeslot API can; the ESP32 has no
  equivalent.) Refs:
  [ESP-IDF NimBLE](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/api-reference/bluetooth/nimble/index.html),
  [sub-1ms BLE sync writeup](https://github.com/lemonforest/mlehaptics/blob/main/docs/Achieving_sub_1ms_time_synchronization_over_BLE_on_ESP32.md),
  [RBS timesync project](https://github.laiyagushi.com/Mirarkitty/mirar-ble-rbs-timesync).
- WiFi + BLE coexistence on the single radio adds 100+ ms of scheduling
  jitter, working against the very thing the ISR tries to measure.
- The connection-event phase is set by the controller's scheduler, not the
  application; the "half-interval" zipper is not app-controllable, and the
  ~16 s H10 startup latency is not deterministic at sub-interval resolution.
- 0.5 ms is below both the ECG sample period (7.69 ms) and the clinical need,
  so it buys nothing.

Conclusion: synchronization belongs in software, anchored on the shared
heartbeat, not in the BLE controller. The invariants in section 2 are exactly
what that software needs.

## 6. Open items / next actions

- Confirm H10 MAC address(es); pre-fill in firmware config.
- ESP-IDF project skeleton: NimBLE single client (Stage 1), then dual client.
- microSD logging format that matches the frame schema; SD backlog resend.
- `/ws/agent` field-agent parity with the schema (agent = `xiao-01`, seq,
  omit raw hex on the wire).
- NTP-over-hotspot at session start; `clock: "unsynced"` fallback.
- Procure the second Polar H10 for Stage 2.
- Decide the archive encoding once Stage 1 wire behaviour is observed.
