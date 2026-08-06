# Elduro - Handover 3 -> 4

Continuing in chat 4? Read this file, then the [README](../../README.md) to
navigate the project. This is an immutable increment - do not revise it; the
next transition gets its own `HANDOVER-4-to-5.md`.

Chat 3 took the project from "planning the field unit" to a **wearable
ESP32-S3 bridge streaming the Polar H10 live to elduro.no**, plus a
productionized HRV/RMSSD view and public HTTPS hosting. The big remaining
field-reliability piece - microSD store-and-forward - is deliberately deferred
to chat 4.

## 1. What chat 3 delivered

- **ESP32-S3 field bridge (bring-up complete).** XIAO ESP32-S3 Sense on
  ESP-IDF v5.4.4 / NimBLE. It BLE-links the H10 and streams raw ECG (130 Hz),
  ACC (200 Hz, including Polar's delta-compressed frames) and native HR/RR
  (0x2A37) to `wss://elduro.no/ws/agent` over the iPhone hotspot, with status
  on the amber GPIO21 LED. BLE + WiFi coexistence on the single radio is proven;
  the full path H10 -> BLE -> ESP32 -> WiFi -> backend -> UI runs with gaps=0.
  Source id `esp32-<mac>:h10`, labelled **ESP32 - Polar H10**. Firmware in
  `firmware/` (commits 685c9fa, d1e080b, f9b995a).
- **HRV/RMSSD productionized.** One shared engine
  (`frontend/src/lib/ecgScope.ts`) drives both the clinical **Raw ECG** scope
  and the **Rhythm / HRV** view (rhythm strip, native-green + ECG-derived-red
  RR tachogram, rolling RMSSD with an uncertainty band, SDNN). Native H10 RR is
  primary; sticky R-peak classification (normal/ectopic); honest "NO SIGNAL"
  blanking.
- **Public hosting.** `elduro.no` (apex) -> FortiGate NAT `62.92.43.149` ->
  raven `10.5.0.22`; Caddy terminates HTTPS (Let's Encrypt) and reverse-proxies
  the web container. Svelte landing page + path routing (`/hr-compare`,
  `/raw-ecg`, `/rhythm-hrv`) with SPA fallback in the backend (commit 853cd02).
  Replaces the tailnet URL for day-to-day use.
- **Agent + backend hardening.** Fixed the capture-agent CPU spin and
  adapter-reorder churn (commit 89461d8). Fixed a backend reconnect race where a
  reconnecting agent was evicted by its own stale task; guarded with a
  per-connection token so the newer connection always wins (commit bd5fdcd) -
  important for WiFi drops on the bike.
- **MCP consolidation.** Moved MCP servers off the laptop onto raven in a new
  repo **KODE15AS/mcp-import** (INWX DNS server live; secrets in
  `~/.config/mcp/*.env` on raven; exposed over Tailscale via supergateway). A
  sibling `mcp-export` repo will hold "export" MCP products (e.g. a Cadify
  configurator) later.
- **Hardware + docs.** Field-unit build plan and dual-H10 sync decision
  (`docs/architecture/field-unit-and-dual-h10-sync.md`); BOM with datasheets
  and now the LiPo spec (`docs/hardware/komponentliste.md`). Evaluated a
  colleague's patient-specific ECG-reconstruction proposal; kept the honest
  "NO SIGNAL" philosophy and deferred the adaptive-LMS idea to the dual-H10
  stage.

## 2. Approaches evaluated and rejected (so chat 4 does not revisit)

- **"ZipSync" hardware-timed dual-H10 synchronization - REJECTED.** Sub-100 us
  HCI-level anchors, deterministic 16 s startup and app-controlled
  connection-event phasing are not achievable on the ESP32-S3 with supported
  APIs, and the precision is clinical overkill. Dual-H10 sync will be
  **heartbeat-anchored in post-processing on the device clocks** instead.
- **Adaptive LMS ECG cleanup with ACC reference - DEFERRED, not rejected.**
  Revisit at the dual-H10 stage (two belts stacked, upper rotated under the
  right nipple, lower under the left) where it aids AV-block classification.
- **MQTT / tailnet transport for the field unit - REJECTED** in favor of a
  token-authed secure WebSocket agent to `wss://elduro.no/ws/agent`, reusing the
  exact JSON frame schema the USB agent already emits.
- **ESP32 replay simulator (chat-2 backlog #4) - SKIPPED.** The hardware
  arrived, so we went straight to real bring-up.

## 3. Field architecture as it stands

```
Polar H10  --BLE (PMD ECG+ACC, HR 0x2A37)-->  XIAO ESP32-S3 Sense
   --WiFi (iPhone Personal Hotspot, 2.4 GHz "Maximize Compatibility")-->
   wss://elduro.no/ws/agent  -->  Caddy  -->  backend (elduro/web)  -->
   /ws/ui  -->  browser (HR Compare / Raw ECG / Rhythm-HRV)
```

- Frame JSON is identical to the USB agent (`t: ecg|acc|hr`, same fields), so
  the views are transport-agnostic. Start/stop is browser-driven with a `mode`:
  `ecg` = ECG+ACC, `hrv` = ECG+ACC+HR, `hr` = HR only.
- LED (GPIO21, active LOW): fast blink = offline, slow = online/idle, solid =
  streaming.
- The raven ASUS BT-600 USB adapter stays the bench reference; the H10 accepts
  one BLE central at a time, so the ESP32 and raven cannot both hold it.

## 4. Decisions locked in chat 3

- Native H10 RR is primary for RMSSD; ECG-derived RR is a cross-check.
- Honest "NO SIGNAL" display - blank rather than show garbage.
- One shared `ecgScope.ts` engine for both ECG views.
- Transport = token-authed WSS agent over the iPhone hotspot; same frame schema
  as the USB agent.
- Backend agent identity via a per-connection token (newest reconnect wins).
- `elduro.no` stays hosted at stw.net DNS -> FortiGate -> raven; Caddy TLS.
- microSD: FAT32 for cards <= 32 GB.
- Dual-H10: optimize one H10 fully first, then add the second; sync is
  heartbeat-anchored server-side.
- Firmware build/flash on raven in Docker (IDF v5.4.4, `/dev/ttyACM0`);
  `firmware/main/wifi_creds.h` is gitignored (see the `.example`).

## 5. Chat-4 backlog (prioritized)

1. **microSD store-and-forward (FatFs) - the top field-reliability gap
   (DEFERRED here on purpose).** No data loss when WiFi drops on Rudskogen.
   Needs the card (ordered, not received), a spill format, upload/resume on
   reconnect, and dedup/merge against live frames on the backend.
2. **Archive encoding (still open from chat 2).** Settle it *together with* the
   SD spill format - they are the same problem. Leaning: per-stream
   append-only JSONL/CBOR session dirs + manifest as canonical; Parquet/EDF as
   export only (see `docs/format/frame-schema.md` section 6).
3. **PSRAM ring buffer** between BLE ingest and the WiFi/SD writers to absorb
   bursts and radio contention.
4. **Real wall-clock time on the ESP32 (SNTP).** Frames currently carry a
   monotonic uptime `ts_host_ns` plus a per-session `elapsed_ms` - fine for one
   live view, but corpus alignment across sessions/devices (and dual-H10 sync)
   needs true time.
5. **Field / mobile UI:** handlebar layout, screen wake lock, aggressive
   reconnect (chat-2 backlog, still open).
6. **Event-marker input:** ACC strap-tap spike as the MVP marker; an optional
   ESP32 button; detailed notes later in the web UI.
7. **Battery / enclosure / power budget** for a 31+ min ride (Grove Base + the
   1S 1000 mAh LiPo now spec'd; cut-off >= 3.3 V; charge via the Grove Base).
8. **HR Compare tab with the ESP32 as source** (`mode: hr`) - verify it works.
9. **Second Polar H10** when it arrives: dual-belt placement, heartbeat-anchored
   sync, then the adaptive-LMS ECG cleanup and AV-block classification.
10. **Kubios one-time validation** of our RMSSD numbers on a bench recording
    (from the chat-2 plan, not yet done).

Known caveat to carry: the H10 needs a ~5-35 s warm-up before the first ECG/HR
frame after start - sensor behaviour, not a bug.

## 6. HRV / RMSSD - now implemented

The chat-2 research (see 2->3, section 6) is now largely built: a
NeuroKit2-style pipeline lives in `analysis/hrv.py` for offline batch work,
while the live views use the browser `ecgScope.ts` engine with native-RR-primary
RMSSD, sticky ectopic classification, and ACC available for motion gating. The
exploratory framing stands: track RMSSD around symptomatic episodes for THIS
subject; it is a research signal, not a validated diagnostic. Still to do:
ACC-gating of live RMSSD windows, the percent-correction badge, and the one-time
Kubios cross-check.

## 7. Housekeeping

- All git/build/deploy on raven over SSH; repo `~/dev/elduro` (remote
  `github-kode15:KODE15AS/elduro`). No local git on the Windows laptop. Mind the
  PowerShell -> bash quoting traps: write scripts to a file and pipe them
  through `tr` + `bash` to strip CR. Markdown docs use CRLF.
- Web deploy: `docker compose build web && docker compose up -d web` (combined
  backend+frontend image `elduro/web`); `docker compose up -d caddy` for TLS.
- Firmware: see `firmware/README.md`. Docker IDF v5.4.4; flash `/dev/ttyACM0`;
  the `cadify` user is in `dialout`.
- Still worth vendoring into `docs/hardware/`: the Espressif ESP32-S3 chip
  datasheet and the Polar PMD service spec from the Polar BLE SDK.
