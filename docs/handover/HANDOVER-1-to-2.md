# Elduro - Handover 1 -> 2

Handover note at the end of chat 1 so a fresh chat can continue without relying
on chat memory. Everything a new session needs should be here or in the code.

**Status at handover:** Phase 1 complete, Phase 2 complete. Live raw ECG and
accelerometer streaming from the Polar H10 works, is validated, and is recorded
losslessly to disk. The on-disk recording format is intentionally **not yet
frozen** - see "Save format (pending)" below.

---

## 1. What Elduro is

A Polar H10 signal lab for research-grade raw biometric capture. It runs as a
Docker container on the **raven** server and is reached over Tailscale. Goal is
to capture the fullest possible raw H10 signal (ECG + motion) with trustworthy
timestamps, so later research work can build on it.

- Repo: `KODE15AS/elduro`
- Web UI (Tailscale, tailnet only): https://cadify104raven.tail14de1b.ts.net:8443
- All git/build/deploy work happens **on raven over SSH**, not on the Windows
  laptop (no local git/gh). See raven access rule.

## 2. Working pattern (Windows -> raven)

- Simple commands: `ssh raven "cd ~/dev/elduro && git status"`.
- Multi-line bash: write a local `.sh` (LF endings) and pipe it:
  `Get-Content script.sh -Raw | ssh raven "tr -d '\r' | bash"`.
  This avoids PowerShell quoting/redirection traps (`$`, nested quotes,
  `2>/dev/null`, CRLF).
- Repo on raven lives at `~/dev/elduro`. Recordings land in
  `~/dev/elduro/recordings/`.

## 3. Deploy / run

```sh
# web container (serves UI + relays WebSocket on :8094)
cd ~/dev/elduro && docker compose up -d --build web

# Linux capture agent (runs on the raven host, needs BlueZ)
docker build -f Dockerfile.agent --target export -o bin .
./bin/elduro-capture --backend ws://127.0.0.1:8094/ws/agent --agent raven

# Windows agent (Lenovo) - cross-compiled, served at /dl/elduro-capture.exe
docker build -f Dockerfile.agent-windows --target export -o bin .
# on the laptop: double-click the .exe (defaults to raven's tailnet IP)
```

## 4. Hardware truths (learned the hard way)

- **ASUS BT-600 USB dongle (`hci1`) is the workhorse.** Use it for all raw
  ECG/ACC capture.
- **Onboard Intel AX211 (`hci0`) is RF-dead for this** (measured RSSI ~ -97 dBm,
  and `le-connection-abort-by-local` on connect). It stays in the UI only as a
  clearly labeled weak option; the agent runs an RSSI preflight and refuses to
  record below -85 dBm.
- Lenovo laptop agent (Windows, WinRT backend) works fine for Heart Rate.
- Test H10: device name `Polar H10 0B052A39`, MAC `24:AC:AC:0B:05:2A`. The H10
  accepts one BLE connection at a time.
- AX211 connects need a scan-stop settle delay + retries; that logic is in the
  agent and should be kept.

## 5. Architecture / file map

- `backend/` - Rust (axum) hub. Binds `0.0.0.0:8080` in dev, `:8094` in the
  container. Routes: `/ws/ui` (browsers) and `/ws/agent` (capture agents).
  Relays commands UI->agent and broadcasts data agent->UI. Forwards a `mode`
  field ("hr" or "ecg") so one agent path serves both phases.
- `capture/` - Rust BLE capture agent (btleplug).
  - `src/main.rs` - scan, connect (with retry), RSSI preflight, session runner,
    lossless recorder.
  - `src/pmd.rs` - Polar PMD protocol: service/char UUIDs, ECG and ACC start
    commands, and frame decoders (ECG 130 Hz microvolts; ACC 200 Hz milli-g,
    including the delta-compressed frame format).
- `frontend/` - Svelte 5 + Vite SPA. Two views:
  - **HR COMPARE** - two lanes: `RAVEN - ASUS BT-600 USB` and `LENOVO - NATIVE
    AGENT`. (The old Web-Bluetooth and AX211 lanes were removed.)
  - **RAW ECG** (default) - `EcgView.svelte`: live scrolling ECG scope + 3-axis
    ACC scope, with a source selector that prefers the BT-600 over the weak
    AX211.
- `Dockerfile` (web), `Dockerfile.agent` (Linux agent), `Dockerfile.agent-windows`
  (cross-compiled .exe), `docker-compose.yml` (web service, mounts `./bin` at
  `/dl/` for the agent download).

## 6. The data (what the streams mean)

- **ECG:** 130 Hz, 14-bit, single-lead, microvolts, 24-bit signed on the wire.
- **ACC:** 200 Hz, 16-bit, +/-8 g, 3-axis [x, y, z] in milli-g.
- Each frame carries the **device nanosecond timestamp** (Polar epoch
  2000-01-01) and the **host receive time**, so multi-sensor alignment is
  possible later.

### Validation evidence (trust the decoder)
- ECG scope shows clean PQRST morphology on the BT-600.
- ACC at rest: acceleration magnitude sqrt(x^2+y^2+z^2) averaged **1012 mg**
  (range 994-1023) over hundreds of samples - i.e. ~1 g of gravity, which proves
  the delta-frame decode, sign extension, and scaling are correct.
- Interleaved ECG+ACC recording observed with **0 gaps**.
- A finger tap on the strap produces a sharp ACC spike - this is the intended
  sync marker for future dual-H10 alignment.

## 7. Save format (PENDING - do not treat as stable)

The current recorder writes a JSONL file per session (header line with device
info and per-stream metadata, then one JSON object per ECG/ACC frame). This was
enough to validate Phase 2, **but the persistent save format and directory
structure are NOT finalized.** Do not build downstream tooling that assumes the
current layout is permanent. Deciding the archival format (and export formats)
is an explicit open task for a later chat - we chose to take more time on it
rather than commit now.

## 8. Next tasks / backlog (for chat 2+)

- **Decide and freeze the save/recording format** (structure + on-disk layout).
- Export path once the format is decided (candidates discussed: CSV for Python,
  EDF for clinical/EDFBrowser tooling) - deliberately deferred, not yet chosen.
- LSL (Lab Streaming Layer) outlet for real-time multi-sensor sync.
- Dual-H10 capture on one person (two simultaneous sensors, time-aligned).

### Research goals this all feeds into
- **Full EKG reconstruction** from the single-lead H10 signal(s).
- **AV block** detection.
- **Inverted T wave** detection.

## 9. Scope note

Phase 3 (offline flash-memory / workout download) is **out of scope** and is not
part of this project. Ignore any earlier mention of it.

