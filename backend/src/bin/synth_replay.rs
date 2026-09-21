//! Offline replay av arkiverte EKG-rammer gjennom syntesemotoren (SynthCore).
//! Brukes til å validere R-topp-matching, klokke-fit og fusjon mot ekte
//! dual-H10-data i MariaDB uten å røre live-kjeden eller arkivet.
//!
//!   ELDURO_DB_URL=mysql://... cargo run -p elduro-backend --bin synth_replay -- \
//!       <fra_unix_ns> <til_unix_ns> <kildeA> <kildeB> [csv-ut]
//!
//! CSV-kolonner: t (syntese-elapsed s), uv (syntetisk µV), basis, conf.
//! I tillegg dumpes rå per-lane-samples til <csv-ut>.lanes.csv for plotting.

#[path = "../synth.rs"]
mod synth;

use mysql_async::{prelude::*, Pool};
use std::io::Write;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("bruk: synth_replay <fra_ns> <til_ns> <kildeA> <kildeB> [csv]");
        std::process::exit(2);
    }
    let from_ns: u64 = args[1].parse().expect("fra_ns");
    let to_ns: u64 = args[2].parse().expect("til_ns");
    let src_a = &args[3];
    let src_b = &args[4];
    let csv_path = args.get(5).cloned().unwrap_or_else(|| "/tmp/synth.csv".into());

    let url = std::env::var("ELDURO_DB_URL").expect("ELDURO_DB_URL må være satt");
    let pool = Pool::new(url.as_str());
    let mut conn = pool.get_conn().await.expect("db-tilkobling");

    // Hent alle EKG-rammer for de to kildene i vinduet, i ankomstrekkefølge -
    // samme rekkefølge som live-kjeden ville matet motoren.
    let rows: Vec<(String, u64, u64, String)> = conn
        .exec(
            "SELECT device_id, ts_device_ns, ts_host_ns, payload
             FROM frames
             WHERE stream = 'ecg' AND device_id IN (?, ?)
               AND ts_host_ns BETWEEN ? AND ?
             ORDER BY ts_host_ns",
            (src_a, src_b, from_ns, to_ns),
        )
        .await
        .expect("frames-spørring");
    println!("replay: {} EKG-rammer i vinduet", rows.len());

    let mut core = synth::SynthCore::new();
    let mut out = std::fs::File::create(&csv_path).expect("csv ut");
    writeln!(out, "t,uv,basis,conf").unwrap();
    let lanes_path = format!("{csv_path}.lanes.csv");
    let mut lanes = std::fs::File::create(&lanes_path).expect("lanes csv");
    writeln!(lanes, "src,ts_device_ns,uv").unwrap();

    let mut n_chunks = 0usize;
    let mut n_samples = 0usize;
    for (i, (src, dev_ns, host_ns, payload)) in rows.iter().enumerate() {
        let samples: Vec<f64> = serde_json::from_str::<Vec<f64>>(payload).unwrap_or_default();
        for (j, uv) in samples.iter().enumerate() {
            // Rå lane-dump med rekonstruert sampletid (siste sample = dev_ns).
            let t_ns = *dev_ns as i64
                - ((samples.len() - 1 - j) as f64 * 1e9 / 130.0) as i64;
            writeln!(lanes, "{src},{t_ns},{uv}").unwrap();
        }
        let chunks = core.ingest_ecg(src, *dev_ns, &samples, *host_ns);
        for c in chunks {
            n_chunks += 1;
            n_samples += c.samples_uv.len();
            let t_last = c.elapsed_s;
            let n = c.samples_uv.len();
            for (k, uv) in c.samples_uv.iter().enumerate() {
                let t = t_last - (n - 1 - k) as f64 / 130.0;
                writeln!(out, "{t:.4},{uv},{},{:.2}", c.basis, c.conf).unwrap();
            }
        }
        if i % 200 == 0 {
            println!("  [{i}] {}", core.debug_line());
        }
    }
    println!("ferdig: {n_chunks} chunks, {n_samples} syntetiske samples");
    println!("slutt-tilstand: {}", core.debug_line());
    println!("csv: {csv_path} + {lanes_path}");
}
