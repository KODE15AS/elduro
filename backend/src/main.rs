mod db;

use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Request, State, WebSocketUpgrade,
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex};
use tower::ServiceExt;
use tower_http::services::ServeDir;

#[derive(Clone, Serialize, Deserialize)]
struct AdapterInfo {
    id: String,
    label: String,
}

struct AgentConn {
    tx: mpsc::UnboundedSender<String>,
    adapters: Vec<AdapterInfo>,
    token: u64,
}

// Distinguishes successive connections that reuse the same agent id, so a
// stale task cannot evict the newer reconnect that replaced it.
static NEXT_AGENT_TOKEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct AppState {
    ui_tx: broadcast::Sender<String>,
    agents: Mutex<HashMap<String, AgentConn>>,
    ingest: db::Ingest,
    // Ønskede økter per kilde (source -> mode), satt ved start og fjernet ved
    // stopp. Lar backend gjensende start når en agent (typisk ESP32-broen)
    // registrerer seg på nytt etter reboot/strømbrudd - feltgjenopptak.
    wanted: Mutex<HashMap<String, String>>,
    // Beltelåsing for dual-H10 (beslutning 21.09.2026): kildemønster -> belte-id
    // fra ELDURO_DEVICE_PINS, f.eks. "esp32-*=0B052A39,raven:hci1=1DA2053E".
    // Mønster med avsluttende '*' er prefiks-match (ESP32-id-en bærer MAC).
    // Kilder låst til ULIKE belter arbitreres per enhet; alt annet beholder
    // global «nyeste start vinner» (19.09-beskyttelsen mot belte-slåssing).
    pins: Vec<(String, String)>,
}

fn parse_pins() -> Vec<(String, String)> {
    std::env::var("ELDURO_DEVICE_PINS")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|e| {
                    let (k, v) = e.split_once('=')?;
                    let (k, v) = (k.trim(), v.trim());
                    (!k.is_empty() && !v.is_empty())
                        .then(|| (k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl AppState {
    /// Belte-id kilden er låst til (fra ELDURO_DEVICE_PINS), om noen.
    fn pin_for(&self, source: &str) -> Option<&str> {
        self.pins
            .iter()
            .find(|(pat, _)| match pat.strip_suffix('*') {
                Some(prefix) => source.starts_with(prefix),
                None => pat == source,
            })
            .map(|(_, belt)| belt.as_str())
    }

    async fn sources_json(&self) -> String {
        let agents = self.agents.lock().await;
        let mut sources = Vec::new();
        for (agent_id, conn) in agents.iter() {
            for a in &conn.adapters {
                sources.push(serde_json::json!({
                    "id": format!("{}:{}", agent_id, a.id),
                    "label": a.label,
                }));
            }
        }
        serde_json::json!({ "t": "sources", "sources": sources }).to_string()
    }

    async fn broadcast_sources(&self) {
        let msg = self.sources_json().await;
        let _ = self.ui_tx.send(msg);
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".into());

    let (ui_tx, _) = broadcast::channel(1024);
    let state = Arc::new(AppState {
        ui_tx,
        agents: Mutex::new(HashMap::new()),
        ingest: db::Ingest::from_env(),
        wanted: Mutex::new(HashMap::new()),
        pins: parse_pins(),
    });
    if !state.pins.is_empty() {
        println!("device pins: {:?}", state.pins);
    }

    // Serve static assets; anything the file server does not find falls back
    // to index.html with a real 200 so client-side deep links (e.g. /raw-ecg)
    // load the SPA instead of a 404.
    let spa_dir = static_dir.clone();
    let spa = tower::service_fn(move |req: Request| {
        let dir = spa_dir.clone();
        async move {
            let served = ServeDir::new(&dir).oneshot(req).await;
            let res: Response = match served {
                Ok(r) if r.status() != StatusCode::NOT_FOUND => r.into_response(),
                _ => {
                    let index = tokio::fs::read(format!("{dir}/index.html"))
                        .await
                        .unwrap_or_default();
                    (
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        index,
                    )
                        .into_response()
                }
            };
            Ok::<Response, std::convert::Infallible>(res)
        }
    });

    let app = Router::new()
        .route("/ws/ui", any(ui_ws))
        .route("/ws/agent", any(agent_ws))
        .fallback_service(spa)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind listener");
    println!("elduro backend listening on :{port}");
    axum::serve(listener, app).await.expect("server error");
}

async fn ui_ws(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ui(socket, state))
}

async fn handle_ui(socket: WebSocket, state: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();

    let snapshot = state.sources_json().await;
    if tx.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }

    let mut bcast = state.ui_tx.subscribe();
    loop {
        tokio::select! {
            m = bcast.recv() => {
                match m {
                    Ok(msg) => {
                        if tx.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            m = rx.next() => {
                let Some(Ok(msg)) = m else { break; };
                if let Message::Text(txt) = msg {
                    handle_ui_command(&state, txt.as_str()).await;
                }
            }
        }
    }
}

async fn handle_ui_command(state: &AppState, raw: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let t = v["t"].as_str().unwrap_or("");
    if t != "start" && t != "stop" {
        return;
    }
    let Some(source) = v["source"].as_str() else {
        return;
    };
    let Some((agent_id, adapter)) = source.split_once(':') else {
        return;
    };
    let agents = state.agents.lock().await;
    // Arbitrering (per enhet fra 21.09.2026, dual-H10): kilder som er låst til
    // ULIKE belter via ELDURO_DEVICE_PINS får strømme samtidig. For alle andre
    // kombinasjoner (samme belte, eller ukjent låsing) gjelder fortsatt global
    // «nyeste start vinner»: H10 godtar én BLE-sentral om gangen, så en start
    // stopper først konkurrerende kilder. Hindrer at to faner/radioer sloss om
    // beltet (sett 19.09).
    if t == "start" {
        let pin_x = state.pin_for(source);
        let mut wanted = state.wanted.lock().await;
        for (other_id, other) in agents.iter() {
            for a in &other.adapters {
                let sid = format!("{other_id}:{}", a.id);
                if sid == source {
                    continue;
                }
                // Begge låst, til hvert sitt belte -> ingen konflikt, la stå.
                if let (Some(px), Some(ps)) = (pin_x, state.pin_for(&sid)) {
                    if px != ps {
                        continue;
                    }
                }
                let stop = serde_json::json!({
                    "t": "stop", "adapter": a.id, "source": sid
                });
                let _ = other.tx.send(stop.to_string());
                // Fjern fra ønsket-lista så de ikke gjenopptar og slåss.
                wanted.remove(&sid);
            }
        }
    }
    if let Some(conn) = agents.get(agent_id) {
        let mut cmd = serde_json::json!({ "t": t, "adapter": adapter, "source": source });
        if let Some(d) = v["duration_s"].as_u64() {
            cmd["duration_s"] = d.into();
        }
        if let Some(m) = v["mode"].as_str() {
            cmd["mode"] = m.into();
        }
        let _ = conn.tx.send(cmd.to_string());
        // Arkiv-ingest + ønsket-økt-bokføring følger kommandostrømmen.
        if t == "start" {
            let mode = v["mode"].as_str().unwrap_or("ecg").to_string();
            state.wanted.lock().await.insert(source.to_string(), mode.clone());
            state.ingest.send(db::Msg::SessionStart { source: source.to_string(), mode });
        } else {
            state.wanted.lock().await.remove(source);
            state.ingest.send(db::Msg::SessionEnd { source: source.to_string() });
        }
    } else {
        let _ = state.ui_tx.send(
            serde_json::json!({
                "t": "status", "source": source, "state": "error",
                "detail": "capture agent not connected"
            })
            .to_string(),
        );
    }
}

// Gjensend start for kilder som har en ønsket økt, når agenten (re)registrerer.
// Dette lar ESP32-broen gjenoppta strømming av seg selv etter reboot/strømbrudd
// uten at brukeren må trykke START på nytt (feltgjenopptak).
async fn resume_wanted(state: &Arc<AppState>, agent_id: &str) {
    // Låserekkefølge: alltid agents før wanted (samme som handle_ui_command),
    // ellers deadlock.
    let agents = state.agents.lock().await;
    let wanted = state.wanted.lock().await;
    let Some(conn) = agents.get(agent_id) else { return };
    for a in &conn.adapters {
        let source = format!("{agent_id}:{}", a.id);
        if let Some(mode) = wanted.get(&source) {
            let cmd = serde_json::json!({
                "t": "start", "adapter": a.id, "source": source, "mode": mode, "duration_s": 0
            });
            let _ = conn.tx.send(cmd.to_string());
            println!("agent '{agent_id}' resume start for {source} (mode={mode})");
        }
    }
}

async fn agent_ws(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent(socket, state))
}

async fn handle_agent(socket: WebSocket, state: Arc<AppState>) {
    let (mut tx, mut rx) = socket.split();

    // First message must be a register.
    let (agent_id, adapters) = loop {
        let Some(Ok(msg)) = rx.next().await else {
            return;
        };
        if let Message::Text(txt) = msg {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(txt.as_str()) else {
                return;
            };
            if v["t"].as_str() != Some("register") {
                return;
            }
            let Some(id) = v["agent"].as_str() else {
                return;
            };
            let adapters: Vec<AdapterInfo> =
                serde_json::from_value(v["adapters"].clone()).unwrap_or_default();
            break (id.to_string(), adapters);
        }
    };

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<String>();
    let my_token = NEXT_AGENT_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state
        .agents
        .lock()
        .await
        .insert(agent_id.clone(), AgentConn { tx: cmd_tx, adapters, token: my_token });
    state.broadcast_sources().await;
    println!("agent '{agent_id}' registered");
    resume_wanted(&state, &agent_id).await;

    loop {
        tokio::select! {
            c = cmd_rx.recv() => {
                let Some(cmd) = c else { break; };
                if tx.send(Message::Text(cmd.into())).await.is_err() {
                    break;
                }
            }
            m = rx.next() => {
                let Some(Ok(msg)) = m else { break; };
                match msg {
                    Message::Text(txt) => {
                        let raw = txt.as_str();
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                            // Agents may re-register when adapters are plugged/unplugged.
                            if v["t"].as_str() == Some("register") {
                                let adapters: Vec<AdapterInfo> =
                                    serde_json::from_value(v["adapters"].clone()).unwrap_or_default();
                                state.agents.lock().await
                                    .entry(agent_id.clone())
                                    .and_modify(|c| c.adapters = adapters);
                                state.broadcast_sources().await;
                                resume_wanted(&state, &agent_id).await;
                                continue;
                            }
                            // Arkiv-ingest: rammer til MariaDB (no-op uten DB),
                            // agent-initierte stopp lukker økten.
                            let msg_t = v["t"].as_str().map(str::to_owned);
                            match msg_t.as_deref() {
                                Some("ecg") | Some("acc") | Some("hr") => {
                                    state.ingest.send(db::Msg::Frame { v });
                                }
                                Some("status") => {
                                    if v["state"].as_str() == Some("stopped") {
                                        if let Some(src) = v["source"].as_str() {
                                            state.ingest.send(db::Msg::SessionEnd {
                                                source: src.to_string(),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        let _ = state.ui_tx.send(raw.to_string());
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }

    let was_current = {
        let mut agents = state.agents.lock().await;
        if agents.get(&agent_id).map(|c| c.token) == Some(my_token) {
            agents.remove(&agent_id);
            true
        } else {
            false
        }
    };
    if was_current {
        state.broadcast_sources().await;
        println!("agent '{agent_id}' disconnected");
    } else {
        println!("agent '{agent_id}' stale connection closed (newer kept)");
    }
}

