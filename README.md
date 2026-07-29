# Elduro

Polar H10 signal lab for research-grade raw biometric capture: live heart rate,
raw single-lead ECG (130 Hz) and 3-axis accelerometer (200 Hz) via Polar's PMD
service, with lossless recording and multi-radio comparison. The next stage
adds a wearable **enduro field unit** (capture while riding, view live on an
iPhone) and a longitudinal **labelled corpus** for model training.

Runs as a Docker container on **raven**, reached over Tailscale.
**Web UI (tailnet only):** https://cadify104raven.tail14de1b.ts.net:8443

## Start here (project map)

This README is the entry point for tracking the whole project.

- **What to do next / current state:** [HANDOVER.md](./HANDOVER.md) - the
  handover index; its CURRENT link is where an new chat should begin.
- **Data contract (frozen):** [docs/format/frame-schema.md](./docs/format/frame-schema.md) -
  frame identity, timestamps, labels, and the two-tier archive model.
- **Field hardware:** [docs/hardware/README.md](./docs/hardware/README.md) -
  inventory, datasheets, power notes for the XIAO ESP32-S3 Sense unit.
- **Code:** `backend/` (Rust axum hub), `capture/` (Rust BLE agent, PMD decoder
  in `src/pmd.rs`), `frontend/` (Svelte + Vite SPA).

## Status

- **Phase 1 - Standard live streaming (complete):** GATT Heart Rate Profile,
  live BPM and R-R intervals.
- **Phase 2 - Advanced live streaming (complete):** raw single-lead ECG
  (130 Hz, microvolts) and 3-axis accelerometer (200 Hz, milli-g) via Polar's
  PMD service, live scopes, lossless recording to disk.
- **Field + corpus stage (planning, chat 3):** wearable ESP32-S3 bridge
  streaming to raven over the iPhone hotspot; canonical archive + regenerated
  training layer; HRV/RMSSD analysis. See the current handover.

The **frame schema is now frozen** ([docs/format/frame-schema.md](./docs/format/frame-schema.md),
schema_version 2). The current `recordings/*.jsonl` files are v1 and will be
migrated; do not treat the v1 on-disk layout as final.

## Architecture

- `backend/` - Rust (axum) hub. Serves the web UI, relays commands to capture
  agents, and broadcasts streams to browsers over WebSocket (`/ws/ui`,
  `/ws/agent`; `:8080` dev, `:8094` container).
- `capture/` - Rust BLE capture agent (btleplug). Registers its adapters with
  the backend and streams Heart Rate Profile data and raw PMD (ECG + ACC) from
  the H10. `src/pmd.rs` holds the Polar PMD protocol and frame decoders.
- `frontend/` - Svelte + Vite SPA: **HR Compare** (per-radio lanes) and
  **Raw ECG** (live ECG + 3-axis ACC scope).
- **Planned field agent:** XIAO ESP32-S3 Sense running a C++/NimBLE port of the
  PMD decoder, SD-card spill, and a WiFi uplink via the iPhone hotspot. The
  raven BT-600 stays the bench reference.

## Signals

- **ECG:** 130 Hz, single-lead, microvolts.
- **ACC:** 200 Hz, +/-8 g, 3-axis in milli-g.
- Frames carry the device nanosecond timestamp (Polar epoch 2000-01-01) plus a
  host receive time for multi-sensor alignment.

Validated: ECG shows clean PQRST morphology; accelerometer magnitude at rest is
~1 g (1012 mg measured), confirming the decoder.

## Radios

- **Raven - ASUS BT-600 USB (`hci1`)** - the workhorse; use for raw ECG/ACC and
  as the field-unit reference.
- **Lenovo - native agent (WinRT)** - works for heart rate.
- Raven onboard Intel AX211 (`hci0`) is RF-too-weak and refuses to record below
  -85 dBm; kept only as a labeled warning option.

The H10 accepts one connection at a time, so capture happens one source at a
time.

## Deploy (raven)

```sh
# web container (port 8094)
docker compose up -d --build web

# Linux capture agent (runs on the host, needs BlueZ)
docker build -f Dockerfile.agent --target export -o bin .
./bin/elduro-capture --backend ws://127.0.0.1:8094/ws/agent --agent raven

# HTTPS for the web UI (tailnet only)
tailscale serve --bg --https=8443 http://127.0.0.1:8094
```

## Lenovo Windows agent

Cross-compile on raven and publish through the web container:

```sh
docker build -f Dockerfile.agent-windows --target export -o bin .
```

The `./bin` directory is mounted into the web container and served at `/dl/`.
On the laptop, download `elduro-capture.exe` (link shown in the UI while no
agent is connected) and double-click it - it defaults to raven's tailnet
backend.

## Dev

- Backend: `cargo run -p elduro-backend` (serves on :8080)
- Frontend: `cd frontend && npm install && npm run dev` (proxies /ws to :8094)
- Agent: `cargo run -p elduro-capture -- --backend ws://127.0.0.1:8080/ws/agent`

All git/build/deploy happens on raven over SSH (repo `~/dev/elduro`); there is
no local git on the Windows laptop.

## Research goals

The raw capture feeds later research: full EKG reconstruction, AV block
detection, inverted T-wave detection (including dual-H10 setups), and
HRV/RMSSD analysis for a myocardial-bridge (vagal) case. Method, libraries and
caveats are in the current handover
([docs/handover/HANDOVER-2-to-3.md](./docs/handover/HANDOVER-2-to-3.md),
section 6).
