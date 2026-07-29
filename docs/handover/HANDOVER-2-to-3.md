# Elduro - Handover 2 -> 3

Finalized at chat-2 close. Starting chat 3? Read this first, then the README.
Previous increment: `HANDOVER-1-to-2.md` (kept in this folder for reference).
Handover increments are immutable once superseded - never revise a past one,
add a new `HANDOVER-N-to-M.md`.

## 1. New direction set in chat 2

The project gained an **enduro field use case**: capture the H10 while riding a
motorcycle on forest trails, process on raven, and view live on an iPhone
(Chrome) with millisecond-timestamped data. This drove a hardware and
architecture pivot, plus a later **ML corpus** extension (2-3 months of
recordings + symptom/condition labels for model training).

## 2. Approaches evaluated and rejected (so chat 3 does not revisit)

- **Sensor Logger app (tszheichoi):** streams phone sensors + decoded BLE HR
  over HTTP/MQTT, but does NOT speak Polar PMD, so NO raw ECG/ACC. A sample
  export confirmed it only logged H10 BLE advertisements (RSSI + Polar
  `manufacturer_data`), no decoded HR/RR, no ECG. Rejected for the core need.
- **XIAO nRF52840 Sense:** BLE-only, NO WiFi. Cannot bridge to the phone
  hotspot. Rejected.
- **LoRa / Meshtastic (Wio-SX1262 + XIAO ESP32S3):** LoRa bandwidth (hundreds
  of bps sustained under EU duty-cycle) cannot carry 130 Hz ECG + 200 Hz ACC.
  Kept on the shelf as a possible phase-2 SAFETY channel (HR + GPS + "alive"
  beacon to a trailhead receiver), not the data path.

## 3. Chosen field architecture

- **XIAO ESP32-S3 Sense** as a BLE-central-to-WiFi bridge: connect to the H10,
  decode PMD (port the Rust logic in `capture/src/pmd.rs` to C++/NimBLE),
  log losslessly to microSD, stream to raven via the **iPhone Personal
  Hotspot** (2.4 GHz; hotspot needs "Maximize Compatibility").
- raven BT-600 (`hci1`) stays the BENCH REFERENCE for verifying the ESP32's
  decoded output byte-for-byte.
- Hardware ordered 2026-07-29; inventory + datasheets in
  `docs/hardware/README.md`. Camera daughter board: never initialize the
  camera (90 mA trap); keep it attached for the microSD slot.
- Power: ~100-140 mA active; 1000-2000 mAh LiPo = a riding day. Bike 12V ->
  5V buck is an option; that makes battery life moot.

## 4. Decisions locked in chat 2

- **Frame schema / identity / timestamp / label model** frozen in
  `docs/format/frame-schema.md` (schema_version 2). Key points: dedup key
  `(device_id, stream, ts_device_ns)`; `subject_id` above device; labels are a
  first-class `annotation` stream (multi-source kept separate; record entry
  latency/method); two-tier storage (immutable canonical archive + regenerated
  training layer); batch-over-live idempotent merge with an explicit
  acceptance test.
- **EKG display:** do NOT downsample the recording (stays full 130 Hz raw).
  Normalize the DISPLAY only. Provide (a) clinical rhythm strip (25 mm/s,
  10 mm/mV grid, live HR), (b) keep raw 130 Hz scope as engineering view,
  (c) later a beat-aligned overlay view for morphology. All rendered from the
  same 130 Hz data.

## 5. Chat-3 backlog

Carried from chat 2 (see item 6 for the HRV work):

1. **Decide the archive encoding** against the section-6 requirements in
   `frame-schema.md` (leaning: session dirs of per-stream append-only
   JSONL/CBOR + manifest as canonical; Parquet/EDF as EXPORT only).
2. **Field transport + reachability.** raven's DNS/connectivity is being
   changed and the ingest endpoint must be **publicly reachable with auth**:
   the ESP32 cannot join the tailnet like the browser can. Likely MQTT over
   WSS (or token-authed WS) exposed to the open internet. Settle before
   firmware.
3. **Deploy ingest on raven:** Mosquitto (or WS ingest) container + a backend
   bridge normalizing incoming frames into the existing UI broadcast.
4. **ESP32 replay simulator** (build before hardware arrives): replay an
   existing recording as a fake `xiao-01` agent over the new transport, with
   injected dropouts + batch re-delivery, to prove ingest + merge/dedup + live
   UI end-to-end. Turns hardware bring-up into a drop-in.
5. **iPhone live view basics:** mobile layout, screen wake lock, aggressive
   reconnect. Minimal but survivable on a handlebar.
6. **ESP32 firmware bring-up:** Arduino vs ESP-IDF decided at bring-up; PMD
   port; SD spill format (section 6.2 of the schema); NTP sync.
7. **R-peak detection as a derived `beats` stream** (Pan-Tompkins or via
   NeuroKit2) on raven: feeds the clinical HR display, beat-overlay view, HRV,
   and ML label alignment. Prerequisite for both the display and the HRV work.
8. **Metadata input:** large one-touch event buttons (palpitation / dizziness
   / chest / marker) as the ESP32 screen concept; strap-tap ACC spike as the
   MVP marker; detailed notes later in the web UI. Screen/power/enclosure is a
   chat-3 hardware call.

## 6. HRV / RMSSD for the myocardial-bridge case (researched in chat 2)

Motivation (subject-specific): myocardial bridge over a major coronary artery,
where vagal (parasympathetic) activity is detrimental, and the practical
non-invasive window onto the vagal arm is time-domain HRV, chiefly **RMSSD**.

### 6.1 What the literature supports

- **RMSSD is the accepted short-term index of vagal (parasympathetic) tone**,
  highly correlated with HF power and relatively respiration-robust. Standards
  and reviews: Task Force of ESC/NASPE, Circulation 1996;93(5):1043; Laborde
  et al., Front Psychol 2017 (PMC5316555, planning/analysis/reporting
  recommendations); Shaffer & Ginsberg, Front Public Health 2017;5:258.
- **Vagal modulation is measurable via HRV in ischemia contexts.** Direct
  peer-reviewed myocardial-bridge<->HRV literature is SPARSE; the closest
  mechanistic support is enhanced vagal modulation with inferoposterior
  exercise-induced ischemia (Nakamura et al., Heart 2006;92(3):325). Honest
  framing for chat 3: tracking RMSSD around symptomatic episodes is a
  reasonable EXPLORATORY signal for this subject, not a validated diagnostic.
  This is exactly why we build the labelled corpus - to look for the subject's
  own RMSSD/symptom associations.
- **The Polar H10 is validated as research-grade for RR/RMSSD**, effectively
  interchangeable with lab ECG for time-domain HRV (concordance >= 0.99,
  MAPE < 1%): Schaffarczyk et al., Sensors 2022;22(17):6536; H10 validity for
  HRV + cardiac autonomic reflex tests (hrvtraining.com, 2026); Gilgen-Ammann
  et al. 2019 (H10 vs Holter). So the sensor is not the limiting factor.

### 6.2 What it takes to produce MEANINGFUL RMSSD (the hard part)

RMSSD is EXTREMELY sensitive to artifacts and ectopy - a single bad interval
can shift it by tens of percent (Kubios; Frontiers 2012 editing review;
Task Force 1996). Consequences for an enduro use case, which is close to the
worst case (motion artifact + possible exercise ectopy):

- **Beat detection must be clean.** Use the raw 130 Hz ECG for R-peak
  detection (our `beats` stream), not just the H10's own RR, so we can inspect
  and re-detect. Keep the H10 RR too as a cross-check.
- **Artifact/ectopic handling is mandatory:** detect and correct ectopic/
  missed/extra beats; prefer INTERPOLATION (cubic-spline or nonlinear
  predictive) over deletion for RMSSD; report the % of intervals corrected;
  discard windows above ~5% correction for short-term RMSSD.
- **Windowing:** classic short-term RMSSD uses ~5-min stationary segments.
  Ultra-short (30-60 s) RMSSD is usable for live display but noisier and must
  be labelled as such.
- **Gate on motion:** use the ACC stream to select low-motion windows (e.g.
  stopped, idling, or steady cruising) for trustworthy RMSSD; flag high-motion
  windows rather than reporting garbage. This ACC-gating is a concrete,
  buildable filter and probably the single most important step for meaningful
  field numbers.
- **Rendering:** live RMSSD as a rolling trend (with a quality/coverage badge),
  plus retrospective per-window RMSSD over the corpus timeline aligned to
  symptom labels.

### 6.3 Suggested libraries (Python, on raven)

- **NeuroKit2** (primary): R-peak detection + `signal_fixpeaks(method="kubios")`
  artifact correction + ~124 HRV indices incl. RMSSD/SD1/pNN50/HF. Most
  comprehensive; heavier deps.
- **hrv-analysis** (aura-healthcare) (cross-check): explicitly validated
  against Kubios v3.1; clean outlier/ectopic removal + interpolation.
- **Kubios HRV Standard** (reference/sanity, not in-pipeline): the de facto
  clinical yardstick to validate our numbers once.
- Lighter options if perf matters on live windows: HeartPy, Systole.
Plan: NeuroKit2 in the derived/training pipeline, hrv-analysis as an
independent check, validate once against Kubios on a bench recording.

## 7. Housekeeping

- All git/build on raven over SSH (see raven access rule). Repo `~/dev/elduro`.
- Still-open doc adds noted earlier: Espressif ESP32-S3 chip datasheet and the
  Polar PMD service spec (from the Polar BLE SDK) into `docs/hardware/`.
