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
}

struct AppState {
    ui_tx: broadcast::Sender<String>,
    agents: Mutex<HashMap<String, AgentConn>>,
}

impl AppState {
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
    });

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
    if let Some(conn) = agents.get(agent_id) {
        let mut cmd = serde_json::json!({ "t": t, "adapter": adapter, "source": source });
        if let Some(d) = v["duration_s"].as_u64() {
            cmd["duration_s"] = d.into();
        }
        if let Some(m) = v["mode"].as_str() {
            cmd["mode"] = m.into();
        }
        let _ = conn.tx.send(cmd.to_string());
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
    state
        .agents
        .lock()
        .await
        .insert(agent_id.clone(), AgentConn { tx: cmd_tx, adapters });
    state.broadcast_sources().await;
    println!("agent '{agent_id}' registered");

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
                        // Agents may re-register when adapters are plugged/unplugged.
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                            if v["t"].as_str() == Some("register") {
                                let adapters: Vec<AdapterInfo> =
                                    serde_json::from_value(v["adapters"].clone()).unwrap_or_default();
                                state.agents.lock().await
                                    .entry(agent_id.clone())
                                    .and_modify(|c| c.adapters = adapters);
                                state.broadcast_sources().await;
                                continue;
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

    state.agents.lock().await.remove(&agent_id);
    state.broadcast_sources().await;
    println!("agent '{agent_id}' disconnected");
}

