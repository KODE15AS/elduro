# Elduro

Polar H10 telemetry lab: compare BLE signal quality across radios, stream live
heart rate, and (later) raw ECG/accelerometer via Polar's PMD service and
offline memory download.

## Architecture

- `backend/` - Rust (axum) hub. Serves the web UI, relays commands to capture
  agents and broadcasts heart rate streams to browsers over WebSocket.
- `capture/` - Rust BLE capture agent (btleplug). Runs on any host with a
  Bluetooth radio (raven, laptop), registers its adapters with the backend and
  streams Heart Rate Profile data from the H10.
- `frontend/` - Svelte + Vite SPA. Four stacked comparison lanes, one per
  radio source. Lane 1 uses Web Bluetooth directly in the browser.

## Lanes

1. Lenovo - Web Bluetooth (browser talks to H10 directly, needs HTTPS)
2. Lenovo - native agent (same capture binary, compiled for Windows)
3. Raven - onboard Intel AX211 (`hci0`)
4. Raven - ASUS BT-600 USB dongle (`hci1`)

The H10 accepts one connection at a time, so lanes record one by one and
freeze their trace for side-by-side comparison.

## Deploy (raven)

```sh
# web container (port 8094)
docker compose up -d --build web

# capture agent (runs on the host, needs BlueZ)
docker build -f Dockerfile.agent --target export -o bin .
./bin/elduro-capture --backend ws://127.0.0.1:8094/ws/agent --agent raven

# HTTPS for Web Bluetooth (tailnet only)
tailscale serve --bg --https=8443 http://127.0.0.1:8094
```

## Lane 2: Windows agent (Lenovo)

Cross-compile on raven and publish through the web container:

```sh
docker build -f Dockerfile.agent-windows --target export -o bin .
docker cp bin/elduro-capture.exe elduro-web-1:/app/static/elduro-capture.exe
```

Then on the laptop (PowerShell):

```powershell
Invoke-WebRequest -Uri https://cadify104raven.tail14de1b.ts.net:8443/elduro-capture.exe -OutFile $env:USERPROFILE\elduro-capture.exe
& $env:USERPROFILE\elduro-capture.exe --backend ws://100.65.19.39:8094/ws/agent --agent lenovo
```

## Dev

- Backend: `cargo run -p elduro-backend` (serves on :8080)
- Frontend: `cd frontend && npm install && npm run dev` (proxies /ws to :8094)
- Agent: `cargo run -p elduro-capture -- --backend ws://127.0.0.1:8080/ws/agent`

