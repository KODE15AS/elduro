use std::error::Error;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
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

#[tokio::main]
async fn main() {
    let mut backend_url = "ws://127.0.0.1:8094/ws/agent".to_string();
    let mut agent_name = "raven".to_string();
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
                            run_session(adapter, source, duration_s, out, stop, my_gen).await;
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

    let mut found: Option<Peripheral> = None;
    let scan_deadline = Instant::now() + Duration::from_secs(SCAN_TIMEOUT_S);
    'scan: while Instant::now() < scan_deadline {
        if is_cancelled(&stop, my_gen) {
            adapter.stop_scan().await.ok();
            send(status(&source, "stopped", "user", None, None));
            return;
        }
        for p in adapter.peripherals().await.unwrap_or_default() {
            if let Ok(Some(props)) = p.properties().await {
                let name = props.local_name.clone().unwrap_or_default();
                if props.services.contains(&HRS_UUID) || name.contains("Polar") {
                    found = Some(p);
                    break 'scan;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
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

    send(status(&source, "connecting", "", Some(&device_name), None));
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

