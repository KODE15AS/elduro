//! MariaDB-ingest av live-rammer (kanonisk arkiv, se
//! docs/format/arkivkoding-vurdering.md). Aktiveres av ELDURO_DB_URL
//! (mysql://bruker:passord@vert:3306/elduro); uten den er alt no-op, så
//! backend kan kjøre uten database til den er provisjonert.
//!
//! Dedup: frames har PRIMARY KEY (device_id, stream, ts_device_ns) og
//! skrives med INSERT IGNORE, så re-leveringer (reconnect, senere
//! SD-spill-opplasting) er idempotente. v1-forbehold: device_id = source
//! inntil belteidentitet følger rammene.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use mysql_async::{prelude::*, Pool};
use tokio::sync::mpsc;

pub enum Msg {
    SessionStart { source: String, mode: String },
    SessionEnd { source: String },
    Frame { v: serde_json::Value },
}

#[derive(Clone)]
pub struct Ingest(Option<mpsc::UnboundedSender<Msg>>);

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

impl Ingest {
    pub fn from_env() -> Self {
        let Ok(url) = std::env::var("ELDURO_DB_URL") else {
            println!("ingest: ELDURO_DB_URL ikke satt - arkivskriving er av");
            return Ingest(None);
        };
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(writer(url, rx));
        println!("ingest: aktiv (MariaDB)");
        Ingest(Some(tx))
    }

    pub fn send(&self, m: Msg) {
        if let Some(tx) = &self.0 {
            let _ = tx.send(m);
        }
    }
}

struct OpenSession {
    id: String,
}

async fn writer(url: String, mut rx: mpsc::UnboundedReceiver<Msg>) {
    let pool = Pool::new(url.as_str());
    let subject = std::env::var("ELDURO_SUBJECT_ID").unwrap_or_else(|_| "subject-1".into());
    let mut open: HashMap<String, OpenSession> = HashMap::new();
    let mut err_count: u64 = 0;

    while let Some(m) = rx.recv().await {
        let res = handle(&pool, &subject, &mut open, m).await;
        if let Err(e) = res {
            err_count += 1;
            if err_count % 50 == 1 {
                eprintln!("ingest-feil (#{err_count}): {e}");
            }
        }
    }
}

async fn ensure_session(
    pool: &Pool,
    subject: &str,
    open: &mut HashMap<String, OpenSession>,
    source: &str,
    mode: &str,
) -> Result<String, mysql_async::Error> {
    if let Some(s) = open.get(source) {
        return Ok(s.id.clone());
    }
    let started = now_ns();
    let agent = source.split(':').next().unwrap_or(source);
    let id = format!("{agent}-{started}");
    let mut conn = pool.get_conn().await?;
    conn.exec_drop(
        "INSERT INTO sessions (session_id, subject_id, agent, source, device_id, mode, started_ns, clock, origin)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'host', 'live')",
        (&id, subject, agent, source, source, mode, started),
    )
    .await?;
    open.insert(source.to_string(), OpenSession { id: id.clone() });
    Ok(id)
}

async fn handle(
    pool: &Pool,
    subject: &str,
    open: &mut HashMap<String, OpenSession>,
    m: Msg,
) -> Result<(), mysql_async::Error> {
    match m {
        Msg::SessionStart { source, mode } => {
            // Nyeste start vinner også her: lukk en eventuell åpen økt og
            // opprett en ny, så mode-bytter gir separate økter.
            if let Some(prev) = open.remove(&source) {
                let mut conn = pool.get_conn().await?;
                conn.exec_drop(
                    "UPDATE sessions SET ended_ns = ? WHERE session_id = ? AND ended_ns IS NULL",
                    (now_ns(), &prev.id),
                )
                .await?;
            }
            ensure_session(pool, subject, open, &source, &mode).await?;
        }
        Msg::SessionEnd { source } => {
            if let Some(prev) = open.remove(&source) {
                let mut conn = pool.get_conn().await?;
                conn.exec_drop(
                    "UPDATE sessions SET ended_ns = ? WHERE session_id = ? AND ended_ns IS NULL",
                    (now_ns(), &prev.id),
                )
                .await?;
            }
        }
        Msg::Frame { v } => {
            let (Some(t), Some(source)) = (v["t"].as_str(), v["source"].as_str()) else {
                return Ok(());
            };
            let stream = t.to_string();
            let source = source.to_string();
            // Rammer kan komme uten at backend så starten (restart av
            // backend midt i en økt): opprett implisitt økt.
            let session_id = ensure_session(pool, subject, open, &source, "ecg").await?;

            let host_ns = v["ts_host_ns"].as_u64().unwrap_or_else(now_ns);
            // HR-rammer har ingen enhetsklokke (0x2A37); bruk mottakstid.
            let dev_ns = v["ts_device_ns"].as_u64().unwrap_or(host_ns);
            let seq = v["seq"].as_u64().unwrap_or(0);
            let (n, payload) = match stream.as_str() {
                "hr" => (
                    1u64,
                    serde_json::json!({ "bpm": v["bpm"], "rr": v["rr"] }).to_string(),
                ),
                _ => {
                    let samples = &v["samples"];
                    let n = samples.as_array().map(|a| a.len() as u64).unwrap_or(0);
                    (n, samples.to_string())
                }
            };
            let mut conn = pool.get_conn().await?;
            conn.exec_drop(
                "INSERT IGNORE INTO frames
                 (device_id, stream, ts_device_ns, session_id, seq, ts_host_ns, n_samples, payload)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (&source, &stream, dev_ns, &session_id, seq, host_ns, n, &payload),
            )
            .await?;
        }
    }
    Ok(())
}
