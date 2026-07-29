# Elduro

Polar H10 signal lab: compare BLE signal quality across radios, stream live
heart rate, and stream raw ECG + accelerometer telemetry via Polar's PMD
service. Runs as a Docker container on **raven**, reached over Tailscale.

**Web UI (Tailscale, tailnet only):** https://cadify104raven.tail14de1b.ts.net:8443

> Continuing in a fresh chat? Start with the handover: [HANDOVER.md](./HANDOVER.md)

## Status

- **Phase 1 - Standard live streaming (complete):** GATT Heart Rate Profile,
  live BPM and R-R intervals.
- **Phase 2 - Advanced live streaming (complete):** raw single-lead ECG
  (130 Hz, microvolts) and 3-axis accelerometer (200 Hz, milli-g) via Polar's
  PMD service, with a live scrolling scope for both and lossless recording to
  disk.

The recording **save format is not yet finalized** - see
[HANDOVER.md](./HANDOVER.md) (section 7). Do not treat the current on-disk
layout as stable.

## Architecture

- `backend/` - Rust (axum) hub. Serves the web UI, relays commands to capture
  agents, and broadcasts streams to browsers over WebSocket (`/ws/ui`,
  `/ws/agent`; `:8080` dev, `:8094` container).
- `capture/` - Rust BLE capture agent (btleplug). Runs on any host with a
  Bluetooth radio, registers its adapters with the backend, and streams both
  Heart Rate Profile data and raw PMD (ECG + ACC) from the H10.
  `src/pmd.rs` holds the Polar PMD protocol and frame decoders.
- `frontend/` - Svelte + Vite SPA with two views: **HR Compare** (per-radio
  lanes) and **Raw ECG** (live ECG + 3-axis ACC scope).

## Signals

- **ECG:** 130 Hz, single-lead, microvolts.
- **ACC:** 200 Hz, +/-8 g, 3-axis in milli-g.
- Frames carry the device nanosecond timestamp (Polar epoch 2000-01-01) plus a
  host receive time for multi-sensor alignment.

Validated: ECG shows clean PQRST morphology; accelerometer magnitude at rest is
~1 g (1012 mg measured), confirming the decoder.

## Radios

- **Raven - ASUS BT-600 USB (`hci1`)** - the workhorse; use for raw ECG/ACC.
- **Lenovo - native agent (WinRT)** - works for heart rate.
- Raven onboard Intel AX211 (`hci0`) is RF-too-weak for this and refuses to
  record below -85 dBm; kept only as a labeled warning option.

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

## Research goals

The raw capture feeds later research work: full EKG reconstruction, AV block
detection, and inverted T-wave detection (including dual-H10 setups on one
person). See [HANDOVER.md](./HANDOVER.md).

