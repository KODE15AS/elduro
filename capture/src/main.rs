mod pmd;

use std::error::Error;
use std::io::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

const HRS_UUID: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);
const HRM_UUID: Uuid = Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);
const BATT_UUID: Uuid = Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);

const SCAN_TIMEOUT_S: u64 = 30;
const CONNECT_ATTEMPTS: u32 = 3;
/// Advertisement RSSI below this is considered too weak for a stable link.
const RSSI_MIN_DBM: i16 = -85;
/// How long to sample advertisement RSSI after the device is first seen.
const RSSI_SAMPLE_S: u64 = 3;

/// On Windows the exe is usually double-clicked on a laptop, so default to
/// the raven backend over the tailnet. On raven itself the agent is started
/// with explicit flags.
#[cfg(windows)]
const DEFAULT_BACKEND: &str = "ws://100.65.19.39:8094/ws/agent";
#[cfg(not(windows))]
const DEFAULT_BACKEND: &str = "ws://127.0.0.1:8094/ws/agent";

fn default_agent_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .map(|h| h.to_lowercase())
        .unwrap_or_else(|_| "agent".to_string())
}

#[tokio::main]
async fn main() {
    let mut backend_url = DEFAULT_BACKEND.to_string();
    let mut agent_name = default_agent_name();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--backend" => backend_url = args[i + 1].clone(),
            "--agent" => agent_name = args[i + 1].clone(),
            _ => {}
        }
        i += 2;
    }

    println!("elduro capture agent '{agent_name}' -> {backend_url}");
    loop {
        match run(&backend_url, &agent_name).await {
            Ok(()) => println!("backend connection closed, reconnecting in 3s"),
            Err(e) => println!("connection error: {e}, retrying in 3s"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn enumerate_adapters(manager: &Manager) -> Vec<(String, String, Adapter)> {
    let mut out = Vec::new();
    let Ok(adapters) = manager.adapters().await else {
        return out;
    };
    for a in adapters {
        let info = a
            .adapter_info()
            .await
            .unwrap_or_else(|_| "unknown".to_string());
        let id = info
            .split_whitespace()
            .next()
            .unwrap_or("adapter")
            .to_string();
        out.push((id, info, a));
    }
    out
}

fn register_msg(agent: &str, adapters: &[(String, String, Adapter)]) -> String {
    let list: Vec<serde_json::Value> = adapters
        .iter()
        .map(|(id, label, _)| serde_json::json!({ "id": id, "label": label }))
        .collect();
    serde_json::json!({ "t": "register", "agent": agent, "adapters": list }).to_string()
}

async fn run(url: &str, agent: &str) -> Result<(), Box<dyn Error>> {
    let (ws, _) = tokio_tungstenite::connect_async(url).await?;
    let (mut tx, mut rx) = ws.split();

    let manager = Manager::new().await?;
    let mut adapters = enumerate_adapters(&manager).await;
    if adapters.is_empty() {
        println!("warning: no bluetooth adapters found");
    }
    tx.send(WsMessage::Text(register_msg(agent, &adapters)))
        .await?;
    println!(
        "registered with {} adapter(s): {:?}",
        adapters.len(),
        adapters.iter().map(|(id, _, _)| id).collect::<Vec<_>>()
    );

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    // Generation counter: bumping it cancels the running session.
    let (stop_tx, stop_rx) = watch::channel(0u64);
    let mut generation = 0u64;
    let mut hotplug = tokio::time::interval(Duration::from_secs(10));
    hotplug.tick().await; // consume immediate first tick

    loop {
        tokio::select! {
            out = out_rx.recv() => {
                let Some(msg) = out else { break; };
                tx.send(WsMessage::Text(msg)).await?;
            }
            _ = hotplug.tick() => {
                let fresh = enumerate_adapters(&manager).await;
                let old_ids: Vec<&String> = adapters.iter().map(|(id, _, _)| id).collect();
                let new_ids: Vec<&String> = fresh.iter().map(|(id, _, _)| id).collect();
                if old_ids != new_ids {
                    println!("adapters changed: {:?} -> {:?}", old_ids, new_ids);
                    adapters = fresh;
                    tx.send(WsMessage::Text(register_msg(agent, &adapters))).await?;
                }
            }
            m = rx.next() => {
                let Some(msg) = m else { break; };
                let msg = msg?;
                let WsMessage::Text(raw) = msg else { continue; };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { continue; };
                match v["t"].as_str() {
                    Some("start") => {
                        let adapter_id = v["adapter"].as_str().unwrap_or("");
                        let source = v["source"].as_str().unwrap_or("").to_string();
                        let duration_s = v["duration_s"].as_u64().unwrap_or(60);
                        // "hr" (Phase 1) or "ecg" (Phase 2 raw stream).
                        let mode = v["mode"].as_str().unwrap_or("hr").to_string();
                        let Some((_, _, adapter)) = adapters.iter().find(|(id, _, _)| id == adapter_id) else {
                            let _ = out_tx.send(status(&source, "error", "adapter not found", None, None));
                            continue;
                        };
                        generation += 1;
                        let _ = stop_tx.send(generation);
                        let adapter = adapter.clone();
                        let out = out_tx.clone();
                        let stop = stop_rx.clone();
                        let my_gen = generation;
                        tokio::spawn(async move {
                            run_session(adapter, source, mode, duration_s, out, stop, my_gen).await;
                        });
                    }
                    Some("stop") => {
                        generation += 1;
                        let _ = stop_tx.send(generation);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn status(
    source: &str,
    state: &str,
    detail: &str,
    device: Option<&str>,
    battery: Option<u8>,
) -> String {
    let mut v = serde_json::json!({ "t": "status", "source": source, "state": state });
    if !detail.is_empty() {
        v["detail"] = detail.into();
    }
    if let Some(d) = device {
        v["device"] = d.into();
    }
    if let Some(b) = battery {
        v["battery"] = b.into();
    }
    v.to_string()
}

fn is_cancelled(stop: &watch::Receiver<u64>, my_gen: u64) -> bool {
    *stop.borrow() != my_gen
}

async fn run_session(
    adapter: Adapter,
    source: String,
    mode: String,
    duration_s: u64,
    out: mpsc::UnboundedSender<String>,
    mut stop: watch::Receiver<u64>,
    my_gen: u64,
) {
    let send = |msg: String| {
        let _ = out.send(msg);
    };

    send(status(&source, "scanning", "looking for Polar H10", None, None));
    if let Err(e) = adapter
        .start_scan(ScanFilter {
            services: vec![HRS_UUID],
        })
        .await
    {
        send(status(&source, "error", &format!("scan failed: {e}"), None, None));
        return;
    }

    // Scan for the device, then keep scanning briefly to sample its
    // advertisement RSSI as a pre-flight signal strength test.
    let mut found: Option<Peripheral> = None;
    let mut best_rssi: Option<i16> = None;
    let mut sample_until: Option<Instant> = None;
    let scan_deadline = Instant::now() + Duration::from_secs(SCAN_TIMEOUT_S);
    loop {
        if is_cancelled(&stop, my_gen) {
            adapter.stop_scan().await.ok();
            send(status(&source, "stopped", "user", None, None));
            return;
        }
        let now = Instant::now();
        match sample_until {
            Some(t) => {
                if now >= t {
                    break;
                }
            }
            None => {
                if now >= scan_deadline {
                    break;
                }
            }
        }
        for p in adapter.peripherals().await.unwrap_or_default() {
            let Ok(Some(props)) = p.properties().await else {
                continue;
            };
            match &found {
                None => {
                    let name = props.local_name.clone().unwrap_or_default();
                    if props.services.contains(&HRS_UUID) || name.contains("Polar") {
                        best_rssi = props.rssi;
                        found = Some(p);
                        sample_until = Some(now + Duration::from_secs(RSSI_SAMPLE_S));
                        send(status(
                            &source,
                            "scanning",
                            "device found, measuring signal strength",
                            None,
                            None,
                        ));
                        break;
                    }
                }
                Some(f) => {
                    if f.id() == p.id() {
                        if let Some(r) = props.rssi {
                            if best_rssi.is_none_or(|b| r > b) {
                                best_rssi = Some(r);
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    adapter.stop_scan().await.ok();

    let Some(p) = found else {
        send(status(
            &source,
            "error",
            "no heart rate device found (strap worn and electrodes moist?)",
            None,
            None,
        ));
        return;
    };
    let device_name = p
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|pr| pr.local_name)
        .unwrap_or_else(|| p.address().to_string());

    if let Some(r) = best_rssi {
        if r < RSSI_MIN_DBM {
            send(status(
                &source,
                "error",
                &format!("signal too weak for recording (RSSI {r} dBm, needs {RSSI_MIN_DBM} dBm or better)"),
                Some(&device_name),
                None,
            ));
            return;
        }
    }
    let rssi_txt = match best_rssi {
        Some(r) => format!("RSSI {r} dBm"),
        None => String::new(),
    };

    send(status(&source, "connecting", &rssi_txt, Some(&device_name), None));
    // Let the controller settle after scanning; connecting immediately after
    // scan-stop is a common trigger for le-connection-abort-by-local on
    // Intel adapters.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut connected = false;
    for attempt in 1..=CONNECT_ATTEMPTS {
        if is_cancelled(&stop, my_gen) {
            send(status(&source, "stopped", "user", None, None));
            return;
        }
        match p.connect().await {
            Ok(()) => {
                connected = true;
                break;
            }
            Err(e) if attempt < CONNECT_ATTEMPTS => {
                send(status(
                    &source,
                    "connecting",
                    &format!("retry {}/{} ({e})", attempt + 1, CONNECT_ATTEMPTS),
                    Some(&device_name),
                    None,
                ));
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
            Err(e) => {
                send(status(&source, "error", &format!("connect failed after {CONNECT_ATTEMPTS} attempts: {e}"), None, None));
                return;
            }
        }
    }
    if !connected {
        return;
    }
    if let Err(e) = p.discover_services().await {
        send(status(&source, "error", &format!("discovery failed: {e}"), None, None));
        p.disconnect().await.ok();
        return;
    }

    let chars = p.characteristics();
    let battery = match chars.iter().find(|c| c.uuid == BATT_UUID) {
        Some(c) => p.read(c).await.ok().and_then(|v| v.first().copied()),
        None => None,
    };

    if mode == "ecg" {
        stream_pmd(&p, &source, &device_name, battery, duration_s, &out, &mut stop, my_gen).await;
        p.disconnect().await.ok();
        return;
    }

    let Some(hrm) = chars.iter().find(|c| c.uuid == HRM_UUID).cloned() else {
        send(status(&source, "error", "no heart rate characteristic", None, None));
        p.disconnect().await.ok();
        return;
    };
    if let Err(e) = p.subscribe(&hrm).await {
        send(status(&source, "error", &format!("subscribe failed: {e}"), None, None));
        p.disconnect().await.ok();
        return;
    }
    let mut notifications = match p.notifications().await {
        Ok(n) => n,
        Err(e) => {
            send(status(&source, "error", &format!("notifications failed: {e}"), None, None));
            p.disconnect().await.ok();
            return;
        }
    };

    send(status(&source, "streaming", "", Some(&device_name), battery));
    let started = Instant::now();
    let effective_s = if duration_s == 0 { 24 * 3600 } else { duration_s };
    let deadline = started + Duration::from_secs(effective_s);
    let reason;

    loop {
        tokio::select! {
            _ = stop.changed() => {
                if is_cancelled(&stop, my_gen) {
                    reason = "user";
                    break;
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                reason = "duration";
                break;
            }
            n = notifications.next() => {
                match n {
                    Some(data) if data.uuid == HRM_UUID => {
                        let (bpm, rr) = parse_hr(&data.value);
                        let msg = serde_json::json!({
                            "t": "hr",
                            "source": source,
                            "ts": started.elapsed().as_millis() as u64,
                            "bpm": bpm,
                            "rr": rr,
                        });
                        send(msg.to_string());
                    }
                    Some(_) => {}
                    None => {
                        reason = "disconnected";
                        break;
                    }
                }
            }
        }
    }

    p.unsubscribe(&hrm).await.ok();
    p.disconnect().await.ok();
    send(status(&source, "stopped", reason, None, None));
}

/// Phase 2: negotiate the PMD service and stream raw 130 Hz ECG plus 200 Hz
/// accelerometer concurrently (both ride the PMD data characteristic and are
/// told apart by their leading measurement-type byte). Forwards decoded
/// frames to the backend for the live view and writes every frame losslessly
/// to disk (device timestamp, host timestamp, raw bytes, decoded values) so
/// nothing is lost before later analysis.
#[allow(clippy::too_many_arguments)]
async fn stream_pmd(
    p: &Peripheral,
    source: &str,
    device_name: &str,
    battery: Option<u8>,
    duration_s: u64,
    out: &mpsc::UnboundedSender<String>,
    stop: &mut watch::Receiver<u64>,
    my_gen: u64,
) {
    let send = |msg: String| {
        let _ = out.send(msg);
    };

    let chars = p.characteristics();
    let Some(control) = chars.iter().find(|c| c.uuid == pmd::PMD_CONTROL).cloned() else {
        send(status(source, "error", "no PMD control point (raw ECG unsupported)", None, None));
        return;
    };
    let Some(data_char) = chars.iter().find(|c| c.uuid == pmd::PMD_DATA).cloned() else {
        send(status(source, "error", "no PMD data characteristic", None, None));
        return;
    };

    // Enable data notifications and control-point indications, then request
    // the ECG measurement start (required) and the ACC stream (best-effort).
    if let Err(e) = p.subscribe(&data_char).await {
        send(status(source, "error", &format!("PMD data subscribe failed: {e}"), None, None));
        return;
    }
    p.subscribe(&control).await.ok();
    if let Err(e) = p
        .write(&control, &pmd::start_ecg_cmd(), WriteType::WithResponse)
        .await
    {
        send(status(source, "error", &format!("PMD ECG start failed: {e}"), None, None));
        p.unsubscribe(&data_char).await.ok();
        return;
    }
    let acc_on = p
        .write(&control, &pmd::start_acc_cmd(), WriteType::WithResponse)
        .await
        .is_ok();

    let mut notifications = match p.notifications().await {
        Ok(n) => n,
        Err(e) => {
            send(status(source, "error", &format!("notifications failed: {e}"), None, None));
            p.unsubscribe(&data_char).await.ok();
            return;
        }
    };

    // Open a lossless recording file next to the agent.
    let mut recorder = PmdRecorder::open(source, device_name);
    let acc_txt = if acc_on { "ECG+ACC" } else { "ECG (ACC unavailable)" };
    if let Some(path) = recorder.path() {
        send(status(source, "streaming", &format!("{acc_txt}, recording to {path}"), Some(device_name), battery));
    } else {
        send(status(source, "streaming", &format!("{acc_txt}, recording unavailable"), Some(device_name), battery));
    }

    let started = Instant::now();
    let effective_s = if duration_s == 0 { 24 * 3600 } else { duration_s };
    let deadline = started + Duration::from_secs(effective_s);
    let mut ecg_total: u64 = 0;
    let mut acc_total: u64 = 0;
    let mut prev_ecg_ts: Option<u64> = None;
    let mut gaps: u64 = 0;
    let reason;

    loop {
        tokio::select! {
            _ = stop.changed() => {
                if is_cancelled(stop, my_gen) {
                    reason = "user";
                    break;
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                reason = "duration";
                break;
            }
            n = notifications.next() => {
                let Some(data) = n else { reason = "disconnected"; break; };
                if data.uuid != pmd::PMD_DATA || data.value.is_empty() {
                    continue;
                }
                let host_unix_ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                match data.value[0] {
                    pmd::MTYPE_ECG => {
                        let Some(frame) = pmd::parse_ecg(&data.value) else { continue; };
                        let n_samples = frame.samples_uv.len() as u64;
                        ecg_total += n_samples;
                        // Detect dropped ECG frames via the device timestamp cadence.
                        if let Some(prev) = prev_ecg_ts {
                            let expected = n_samples * pmd::ECG_PERIOD_NS;
                            if frame.timestamp_ns.saturating_sub(prev) > expected + pmd::ECG_PERIOD_NS {
                                gaps += 1;
                            }
                        }
                        prev_ecg_ts = Some(frame.timestamp_ns);
                        recorder.write_ecg(host_unix_ns, &frame, &data.value);
                        send(serde_json::json!({
                            "t": "ecg",
                            "source": source,
                            "ts_device_ns": frame.timestamp_ns,
                            "ts_host_ns": host_unix_ns,
                            "elapsed_ms": started.elapsed().as_millis() as u64,
                            "samples": frame.samples_uv,
                            "total": ecg_total,
                            "gaps": gaps,
                        }).to_string());
                    }
                    pmd::MTYPE_ACC => {
                        let Some(frame) = pmd::parse_acc(&data.value) else { continue; };
                        acc_total += frame.samples_mg.len() as u64;
                        recorder.write_acc(host_unix_ns, &frame, &data.value);
                        send(serde_json::json!({
                            "t": "acc",
                            "source": source,
                            "ts_device_ns": frame.timestamp_ns,
                            "ts_host_ns": host_unix_ns,
                            "samples": frame.samples_mg,
                            "total": acc_total,
                        }).to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    p.write(&control, &pmd::stop_cmd(pmd::MTYPE_ECG), WriteType::WithResponse).await.ok();
    if acc_on {
        p.write(&control, &pmd::stop_cmd(pmd::MTYPE_ACC), WriteType::WithResponse).await.ok();
    }
    p.unsubscribe(&data_char).await.ok();
    p.unsubscribe(&control).await.ok();
    recorder.close();
    send(status(source, "stopped", reason, None, None));
}

/// Appends raw PMD frames to a JSONL file (one JSON object per frame). Each
/// line keeps the stream kind, the device timestamp, the host arrival time,
/// the raw BLE bytes as hex, and the decoded values, so the recording is
/// lossless for both the ECG and accelerometer streams.
struct PmdRecorder {
    writer: Option<std::io::BufWriter<std::fs::File>>,
    path: Option<String>,
}

impl PmdRecorder {
    fn open(source: &str, device_name: &str) -> Self {
        if std::fs::create_dir_all("recordings").is_err() {
            return Self { writer: None, path: None };
        }
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let safe_source = source.replace([':', '/', '\\'], "-");
        let path = format!("recordings/pmd_{safe_source}_{epoch}.jsonl");
        let Ok(file) = std::fs::File::create(&path) else {
            return Self { writer: None, path: None };
        };
        let mut writer = std::io::BufWriter::new(file);
        let header = serde_json::json!({
            "type": "elduro-pmd-recording",
            "version": 2,
            "source": source,
            "device": device_name,
            "streams": {
                "ecg": { "sample_rate_hz": 130, "resolution_bits": 14, "unit": "microvolt" },
                "acc": { "sample_rate_hz": 200, "resolution_bits": 16, "range_g": 8, "unit": "milli_g" }
            },
            "device_epoch": "2000-01-01T00:00:00Z",
            "started_unix_ns": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0),
        });
        let _ = writeln!(writer, "{header}");
        Self { writer: Some(writer), path: Some(path) }
    }

    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    fn write_ecg(&mut self, host_unix_ns: u64, frame: &pmd::EcgFrame, raw: &[u8]) {
        let Some(writer) = self.writer.as_mut() else { return };
        let raw_hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let line = serde_json::json!({
            "s": "ecg",
            "ts_device_ns": frame.timestamp_ns,
            "ts_host_ns": host_unix_ns,
            "n": frame.samples_uv.len(),
            "uv": frame.samples_uv,
            "raw": raw_hex,
        });
        let _ = writeln!(writer, "{line}");
    }

    fn write_acc(&mut self, host_unix_ns: u64, frame: &pmd::AccFrame, raw: &[u8]) {
        let Some(writer) = self.writer.as_mut() else { return };
        let raw_hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let line = serde_json::json!({
            "s": "acc",
            "ts_device_ns": frame.timestamp_ns,
            "ts_host_ns": host_unix_ns,
            "n": frame.samples_mg.len(),
            "mg": frame.samples_mg,
            "raw": raw_hex,
        });
        let _ = writeln!(writer, "{line}");
    }

    fn close(&mut self) {
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.flush();
        }
    }
}

/// Parse a standard Heart Rate Measurement (0x2A37) payload.
/// Returns (bpm, rr_intervals_ms).
fn parse_hr(data: &[u8]) -> (u16, Vec<u32>) {
    if data.is_empty() {
        return (0, vec![]);
    }
    let flags = data[0];
    let mut i = 1usize;
    let bpm: u16 = if flags & 0x01 != 0 {
        if data.len() < 3 {
            return (0, vec![]);
        }
        let v = u16::from_le_bytes([data[1], data[2]]);
        i = 3;
        v
    } else {
        if data.len() < 2 {
            return (0, vec![]);
        }
        i = 2;
        data[1] as u16
    };
    // Energy expended present: skip 2 bytes.
    if flags & 0x08 != 0 {
        i += 2;
    }
    let mut rr = Vec::new();
    if flags & 0x10 != 0 {
        while i + 2 <= data.len() {
            let raw = u16::from_le_bytes([data[i], data[i + 1]]);
            // RR is in units of 1/1024 s; convert to ms.
            rr.push((raw as u32 * 1000) / 1024);
            i += 2;
        }
    }
    (bpm, rr)
}

