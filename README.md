# Elduro

Polar H10 signal lab for research-grade raw biometric capture: live heart rate,
raw single-lead ECG (130 Hz) and 3-axis accelerometer (200 Hz) via Polar's PMD
service, with lossless recording and multi-radio comparison. The next stage
adds a wearable **enduro field unit** (capture while riding, view live on an
iPhone) and a longitudinal **labelled corpus** for model training.

Runs as a Docker container on **raven**. **Web UI:** https://elduro.no
(public, HTTPS via Caddy). The Tailscale URL
(https://cadify104raven.tail14de1b.ts.net:8443) still works on the tailnet.

## Start here (project map)

This README is the entry point for tracking the whole project.

- **What to do next / current state:** [HANDOVER.md](./HANDOVER.md) - the
  handover index; its CURRENT link is where an new chat should begin.
- **Data contract (frozen):** [docs/format/frame-schema.md](./docs/format/frame-schema.md) -
  frame identity, timestamps, labels, and the two-tier archive model.
- **Field hardware:** [docs/hardware/README.md](./docs/hardware/README.md) -
  inventory, datasheets, power notes for the XIAO ESP32-S3 Sense unit.
- **Code:** `backend/` (Rust axum hub), `capture/` (Rust BLE agent, PMD decoder
  in `src/pmd.rs`), `frontend/` (Svelte + Vite SPA), `firmware/` (ESP32-S3
  field bridge, ESP-IDF/NimBLE).

## Status

- **Phase 1 - Standard live streaming (complete):** GATT Heart Rate Profile,
  live BPM and R-R intervals.
- **Phase 2 - Advanced live streaming (complete):** raw single-lead ECG
  (130 Hz, microvolts) and 3-axis accelerometer (200 Hz, milli-g) via Polar's
  PMD service, live scopes, lossless recording to disk.
- **Field + corpus stage (in progress, chat 3):** the wearable XIAO ESP32-S3
  bridge streams the H10 (ECG + ACC + native HR/RR) to raven over the iPhone
  hotspot and appears in the UI as **ESP32 - Polar H10**; the live path is
  proven end-to-end. HRV/RMSSD analysis is productionized (a shared clinical
  scope plus the Rhythm / HRV view). Still open: microSD store-and-forward and
  the canonical archive encoding. See the current handover.

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
- `frontend/` - Svelte + Vite SPA: a landing page plus three tools -
  **HR Compare** (per-radio lanes), **Raw ECG** (clinical live ECG + 3-axis ACC
  scope), and **Rhythm / HRV** (rhythm strip, native + ECG-derived RR tachogram,
  rolling RMSSD). Shared ECG engine in `src/lib/ecgScope.ts`; path-routed under
  elduro.no.
- `firmware/` - **field agent (bring-up complete):** XIAO ESP32-S3 Sense
  running an ESP-IDF/NimBLE port of the PMD decoder. It links to the H10 over
  BLE and streams ECG + ACC + native HR/RR to `wss://elduro.no/ws/agent` over
  the iPhone hotspot, with an amber GPIO21 status LED (fast blink = offline,
  slow = online/idle, solid = streaming). microSD store-and-forward is the next
  step. The raven BT-600 stays the bench reference.

## Signals

- **ECG:** 130 Hz, single-lead, microvolts.
- **ACC:** 200 Hz, +/-8 g, 3-axis in milli-g.
- Frames carry the device nanosecond timestamp (Polar epoch 2000-01-01) plus a
  host receive time for multi-sensor alignment.

Validated: ECG shows clean PQRST morphology; accelerometer magnitude at rest is
~1 g (1012 mg measured), confirming the decoder.

## Radios

- **ESP32 - Polar H10** - the wearable XIAO ESP32-S3 field bridge over WiFi;
  streams ECG + ACC + native HR/RR. This is the field unit.
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

# public HTTPS at elduro.no (Caddy reverse proxy, automatic Let's Encrypt)
docker compose up -d caddy
# the Tailscale URL still works on the tailnet:
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
caveats are in the current handover (see [HANDOVER.md](./HANDOVER.md)).
