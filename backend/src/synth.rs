//! Syntetisk estimert EKG fra to Polar H10 (dual-H10, metode 1 fra
//! docs/architecture/syntetisk-ekg-dual-h10.md, besluttet 21.09.2026):
//! R-topp-forankret tidsjustering mellom beltenes enhetsklokker (offset +
//! krystalldrift via lineær regresjon over matchede R-topper), deretter en
//! polaritetsjustert, vektet kombinasjon av de to avledningene.
//!
//! Kjernen (`SynthCore`) er bevisst ren/synkron (ingen tokio, ingen IO) slik
//! at den kan replay-testes offline mot arkivet i MariaDB (bin/synth_replay).
//! main.rs mater den med 'ecg'-rammer fra agentene og kringkaster de
//! returnerte syntetiske rammene til UI-et som `source: "synth"`.
//!
//! Ærlighetsprinsipp (Jørn 21.09): dette er et ESTIMAT ("syntetisk estimert
//! EKG"), ikke hjertets sanne EKG - to avledninger gir en plan projeksjon,
//! ikke 3D. Med bare én aktiv strøm sendes den ene avledningen uendret
//! gjennom, merket `basis` deretter.

use std::collections::VecDeque;

const ECG_FS: f64 = 130.0;
const SAMPLE_DT: f64 = 1.0 / ECG_FS;
/// Baseline-EMA-tidskonstant (s) - speiler frontendens ecgScope.
const BASE_TAU: f64 = 0.75;
/// Ringbufferlengde per belte (s).
const LANE_BUF_S: f64 = 30.0;
/// Minste avstand mellom to R-topper (s).
const MIN_RR_S: f64 = 0.3;
/// Matchevindu før/etter konvergert fit (s).
const GATE_BOOT_S: f64 = 0.25;
const GATE_FIT_S: f64 = 0.08;
/// Antall R-topp-par i regresjonsvinduet.
const MAX_PAIRS: usize = 240;
/// Guard mot å fusjonere helt ferske samples (la begge belter levere).
const EMIT_GUARD_S: f64 = 0.15;
/// En lane regnes som død etter så mange sekunder uten rammer (vertsklokke).
const LANE_STALE_S: f64 = 5.0;
/// Eierskifte (tidslinje) etter så lang stillhet fra eieren.
const OWNER_STALE_S: f64 = 10.0;

fn base_alpha() -> f64 {
    1.0 - (-1.0 / (ECG_FS * BASE_TAU)).exp()
}

/// Én R-topp på beltets egen enhetsklokke (sekunder, med lane-t0 trukket fra).
#[derive(Clone, Copy)]
struct Peak {
    t: f64,
}

struct Lane {
    source: String,
    /// Samples: (enhetstid s, baseline-fjernet mV). Monotont stigende t.
    ts: VecDeque<f64>,
    ys: VecDeque<f32>,
    /// Første enhets-ns sett for denne lane (nullpunkt for t).
    t0_ns: Option<u64>,
    last_t: f64,
    base_ema: f64,
    base_init: bool,
    // R-topp-deteksjon (adaptiv terskel, lokalt maksimum, speiler ecgScope).
    recent_max: f64,
    last_peak_t: f64,
    peaks: VecDeque<Peak>,
    /// Vertsklokke (ns) ved siste ramme - freshness + bootstrap-anker.
    last_host_ns: u64,
    /// Enhetstid ved siste ramme (for bootstrap-offset via vertsklokka).
    last_dev_t: f64,
}

impl Lane {
    fn new(source: &str) -> Self {
        Lane {
            source: source.to_string(),
            ts: VecDeque::new(),
            ys: VecDeque::new(),
            t0_ns: None,
            last_t: f64::NEG_INFINITY,
            base_ema: 0.0,
            base_init: false,
            recent_max: 0.0,
            last_peak_t: f64::NEG_INFINITY,
            peaks: VecDeque::new(),
            last_host_ns: 0,
            last_dev_t: 0.0,
        }
    }

    fn fresh(&self, now_host_ns: u64) -> bool {
        self.last_host_ns > 0
            && (now_host_ns.saturating_sub(self.last_host_ns) as f64) / 1e9 < LANE_STALE_S
    }

    fn newest_t(&self) -> f64 {
        self.last_t
    }

    /// Ta inn én EKG-ramme (µV-samples, enhets-ns for SISTE sample).
    fn ingest(&mut self, ts_device_ns: u64, samples_uv: &[f64], host_ns: u64) {
        if samples_uv.is_empty() {
            return;
        }
        let t0 = *self.t0_ns.get_or_insert(ts_device_ns);
        // Enhetsklokka hopper aldri bakover i samme økt; et stort bakoverhopp
        // betyr belte-reset (batteriuttak) -> nullstill lane.
        let t_last = (ts_device_ns as f64 - t0 as f64) / 1e9;
        if t_last < self.last_t - 3.0 {
            *self = Lane::new(&self.source);
            return self.ingest(ts_device_ns, samples_uv, host_ns);
        }
        let n = samples_uv.len();
        let mut base = t_last - (n as f64 - 1.0) * SAMPLE_DT;
        // Forward-only shift (samme prinsipp som frontendens ecgTimeBase).
        if self.last_t > f64::NEG_INFINITY {
            let min_base = self.last_t + SAMPLE_DT - (n as f64 - 1.0) * SAMPLE_DT;
            if base < min_base {
                base = min_base;
            }
        }
        let alpha = base_alpha();
        for (j, uv) in samples_uv.iter().enumerate() {
            let mv = uv / 1000.0;
            if !self.base_init {
                self.base_ema = mv;
                self.base_init = true;
            } else {
                let innov = (mv - self.base_ema).clamp(-0.4, 0.4);
                self.base_ema += alpha * innov;
            }
            let t = base + j as f64 * SAMPLE_DT;
            self.ts.push_back(t);
            self.ys.push_back((mv - self.base_ema) as f32);
        }
        self.last_t = base + (n as f64 - 1.0) * SAMPLE_DT;
        self.last_host_ns = host_ns;
        self.last_dev_t = self.last_t;
        // Trim ring + peaks.
        let cut = self.last_t - LANE_BUF_S;
        while self.ts.front().is_some_and(|&t| t < cut) {
            self.ts.pop_front();
            self.ys.pop_front();
        }
        while self.peaks.front().is_some_and(|p| p.t < cut) {
            self.peaks.pop_front();
        }
        self.detect_peaks();
    }

    /// Adaptiv R-topp-deteksjon over det nyeste av bufferet.
    fn detect_peaks(&mut self) {
        let n = self.ts.len();
        if n < 5 {
            return;
        }
        // Skann bare hale vi ikke har sett: fra siste topp (eller starten).
        let guard_t = self.last_t - 2.0 * SAMPLE_DT;
        let start_t = self.last_peak_t.max(self.last_t - 3.0);
        // Finn startindeks (lineært bakfra er billig: buffer er kort).
        let mut i = n.saturating_sub(2);
        while i > 2 && self.ts[i] > start_t {
            i -= 1;
        }
        while i + 2 < n {
            let t = self.ts[i];
            if t > guard_t {
                break;
            }
            let y = self.ys[i] as f64;
            self.recent_max = y.max(self.recent_max * 0.999);
            let thr = (0.45 * self.recent_max).max(0.18);
            let is_max = y >= thr
                && y >= self.ys[i - 1] as f64
                && y >= self.ys[i - 2] as f64
                && y >= self.ys[i + 1] as f64
                && y >= self.ys[i + 2] as f64;
            if is_max && t - self.last_peak_t > MIN_RR_S {
                self.peaks.push_back(Peak { t });
                self.last_peak_t = t;
            }
            i += 1;
        }
    }

    /// Lineær interpolasjon av signalverdien ved enhetstid t.
    fn sample_at(&self, t: f64) -> Option<f32> {
        if self.ts.is_empty() || t < *self.ts.front()? || t > *self.ts.back()? {
            return None;
        }
        // Binærsøk etter t (VecDeque er sortert).
        let (mut lo, mut hi) = (0usize, self.ts.len() - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.ts[mid] <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (t0, t1) = (self.ts[lo], self.ts[hi]);
        let (y0, y1) = (self.ys[lo] as f64, self.ys[hi] as f64);
        if t1 <= t0 {
            return Some(y0 as f32);
        }
        let f = (t - t0) / (t1 - t0);
        Some((y0 + f * (y1 - y0)) as f32)
    }
}

/// Lineær fit tA = a + b*tB over matchede R-topp-par.
struct ClockFit {
    pairs: VecDeque<(f64, f64)>, // (tB, tA)
    a: f64,
    b: f64,
    ready: bool,
    residual_ms: f64,
    matched_recent: VecDeque<bool>,
}

impl ClockFit {
    fn new() -> Self {
        ClockFit {
            pairs: VecDeque::new(),
            a: 0.0,
            b: 1.0,
            ready: false,
            residual_ms: f64::NAN,
            matched_recent: VecDeque::new(),
        }
    }

    fn map_b_to_a(&self, t_b: f64) -> f64 {
        self.a + self.b * t_b
    }
    fn map_a_to_b(&self, t_a: f64) -> f64 {
        (t_a - self.a) / self.b
    }

    fn add_pair(&mut self, t_b: f64, t_a: f64) {
        self.pairs.push_back((t_b, t_a));
        while self.pairs.len() > MAX_PAIRS {
            self.pairs.pop_front();
        }
        if self.pairs.len() >= 3 {
            let n = self.pairs.len() as f64;
            let (mut sb, mut sa) = (0.0, 0.0);
            for &(tb, ta) in &self.pairs {
                sb += tb;
                sa += ta;
            }
            let (mb, ma) = (sb / n, sa / n);
            let (mut cov, mut var) = (0.0, 0.0);
            for &(tb, ta) in &self.pairs {
                cov += (tb - mb) * (ta - ma);
                var += (tb - mb) * (tb - mb);
            }
            if var > 1e-9 {
                // Krystalldrift er ppm-nivå; klem stigningen så et skjevt
                // tidlig-vindu ikke kan gi en fysisk umulig fit.
                self.b = (cov / var).clamp(0.999, 1.001);
                self.a = ma - self.b * mb;
                let mut ss = 0.0;
                for &(tb, ta) in &self.pairs {
                    let r = ta - self.map_b_to_a(tb);
                    ss += r * r;
                }
                self.residual_ms = (ss / n).sqrt() * 1000.0;
                self.ready = true;
            }
        }
    }

    fn note_match(&mut self, hit: bool) {
        self.matched_recent.push_back(hit);
        while self.matched_recent.len() > 20 {
            self.matched_recent.pop_front();
        }
    }

    fn match_rate(&self) -> f64 {
        if self.matched_recent.is_empty() {
            return 0.0;
        }
        self.matched_recent.iter().filter(|&&x| x).count() as f64
            / self.matched_recent.len() as f64
    }
}

/// Én ferdig syntetisk ramme klar for kringkasting.
pub struct SynthChunk {
    /// µV-samples (avrundet), 130 Hz.
    pub samples_uv: Vec<i32>,
    /// Tidslinje for SISTE sample, sekunder siden syntese-øktstart.
    pub elapsed_s: f64,
    /// Grunnlag: "A+B", eller kildenavnet ved solo.
    pub basis: String,
    /// 0..1 - styrer "washed out"-visningen i UI-et.
    pub conf: f64,
    pub offset_ms: f64,
    pub drift_ppm: f64,
    pub residual_ms: f64,
    pub total: u64,
}

pub struct SynthCore {
    owner: Option<Lane>,
    other: Option<Lane>,
    fit: ClockFit,
    /// Polaritet for other-lane i fusjonen (+1/-1), EMA-styrt korrelasjon.
    corr_ema: f64,
    /// Siste eier-tid vi har emittert til.
    emitted_until: f64,
    /// Nullpunkt for utgående elapsed-tidslinje (eier-tid).
    out_t0: Option<f64>,
    total_out: u64,
    /// Matchet siste B-topp-indeks (unngå dobbeltmatching).
    last_matched_b_peak_t: f64,
}

impl Default for SynthCore {
    fn default() -> Self {
        Self::new()
    }
}

impl SynthCore {
    pub fn new() -> Self {
        SynthCore {
            owner: None,
            other: None,
            fit: ClockFit::new(),
            corr_ema: 0.0,
            emitted_until: f64::NEG_INFINITY,
            out_t0: None,
            total_out: 0,
            last_matched_b_peak_t: f64::NEG_INFINITY,
        }
    }

    /// Foretrukket tidslinjeeier: ESP32-broen (belte A i benkoppsettet).
    fn prefers_owner(source: &str) -> bool {
        source.starts_with("esp32-")
    }

    /// Mat inn én 'ecg'-ramme. Returnerer 0..n ferdige syntetiske chunks.
    pub fn ingest_ecg(
        &mut self,
        source: &str,
        ts_device_ns: u64,
        samples_uv: &[f64],
        host_ns: u64,
    ) -> Vec<SynthChunk> {
        // Rut til riktig lane; opprett ved behov.
        let is_owner = match (&self.owner, &self.other) {
            (Some(o), _) if o.source == source => true,
            (_, Some(x)) if x.source == source => false,
            (None, _) => {
                self.owner = Some(Lane::new(source));
                true
            }
            (Some(o), None) => {
                // Andre kilde: hvis den nye er ESP32 og eieren ikke er det,
                // bytt roller (eier = tidslinje). Skjer bare før fit.
                if Self::prefers_owner(source) && !Self::prefers_owner(&o.source) {
                    self.other = self.owner.take();
                    self.owner = Some(Lane::new(source));
                    self.fit = ClockFit::new();
                    true
                } else {
                    self.other = Some(Lane::new(source));
                    false
                }
            }
            _ => return Vec::new(), // tredje kilde: ignorer i v1
        };
        if is_owner {
            if let Some(o) = self.owner.as_mut() {
                o.ingest(ts_device_ns, samples_uv, host_ns);
            }
        } else if let Some(x) = self.other.as_mut() {
            x.ingest(ts_device_ns, samples_uv, host_ns);
        }

        // Eierskifte hvis eieren har vært død lenge og den andre lever.
        if let (Some(o), Some(x)) = (&self.owner, &self.other) {
            if !o.fresh(host_ns)
                && x.fresh(host_ns)
                && (host_ns.saturating_sub(o.last_host_ns) as f64) / 1e9 > OWNER_STALE_S
            {
                self.other = self.owner.take();
                self.owner = self.other.take(); // swap via take
                std::mem::swap(&mut self.owner, &mut self.other);
                // NB: enkel swap; fit og utgående tidslinje må bygges på nytt.
                self.fit = ClockFit::new();
                self.out_t0 = None;
                self.emitted_until = f64::NEG_INFINITY;
            }
        }

        self.match_peaks();
        self.emit(host_ns)
    }

    /// Match ferske B-topper mot A-topper og oppdater klokke-fiten.
    fn match_peaks(&mut self) {
        let (Some(o), Some(x)) = (&self.owner, &self.other) else {
            return;
        };
        // Bootstrap-offset via vertsklokka til fit er klar.
        let boot_a = if self.fit.ready {
            None
        } else {
            let dt_host = (o.last_host_ns as f64 - x.last_host_ns as f64) / 1e9;
            Some((o.last_dev_t - dt_host) - x.last_dev_t)
        };
        let gate = if self.fit.ready { GATE_FIT_S } else { GATE_BOOT_S };
        let mut new_pairs: Vec<(f64, f64)> = Vec::new();
        let mut hits: Vec<bool> = Vec::new();
        for pb in x.peaks.iter() {
            if pb.t <= self.last_matched_b_peak_t {
                continue;
            }
            // Ikke match helt ferske topper; A-toppen kan mangle ennå.
            let pred = if self.fit.ready {
                self.fit.map_b_to_a(pb.t)
            } else {
                pb.t + boot_a.unwrap_or(0.0)
            };
            if pred > o.newest_t() - 0.3 {
                break;
            }
            let mut best: Option<(f64, f64)> = None; // (|err|, tA)
            for pa in o.peaks.iter() {
                let err = (pa.t - pred).abs();
                if err < gate && best.is_none_or(|(be, _)| err < be) {
                    best = Some((err, pa.t));
                }
            }
            self.last_matched_b_peak_t = pb.t;
            match best {
                Some((_, ta)) => {
                    new_pairs.push((pb.t, ta));
                    hits.push(true);
                }
                None => hits.push(false),
            }
        }
        for (tb, ta) in new_pairs {
            self.fit.add_pair(tb, ta);
        }
        for h in hits {
            self.fit.note_match(h);
        }
    }

    /// Emitter syntetiske samples opp til felles trygg horisont.
    fn emit(&mut self, now_host_ns: u64) -> Vec<SynthChunk> {
        let Some(o) = &self.owner else {
            return Vec::new();
        };
        let o_fresh = o.fresh(now_host_ns);
        let x_fresh = self.other.as_ref().is_some_and(|x| x.fresh(now_host_ns));
        let dual = o_fresh && x_fresh && self.fit.ready;

        // Horisont på eier-tidslinjen.
        let horizon = if dual {
            let x = self.other.as_ref().unwrap();
            (o.newest_t()).min(self.fit.map_b_to_a(x.newest_t())) - EMIT_GUARD_S
        } else if o_fresh {
            o.newest_t() - EMIT_GUARD_S
        } else {
            return Vec::new(); // solo-B uten eierskifte håndteres av stale-swap
        };

        // Ny økt / stort hopp: re-anker utgående tidslinje.
        if self.out_t0.is_none() || self.emitted_until == f64::NEG_INFINITY {
            self.out_t0 = Some(horizon.min(o.newest_t()) - 0.5);
            self.emitted_until = self.out_t0.unwrap();
        }
        if horizon <= self.emitted_until {
            return Vec::new();
        }
        // Hvis vi har sakket mer enn bufferet akter, hopp frem.
        if horizon - self.emitted_until > LANE_BUF_S - 5.0 {
            self.emitted_until = horizon - 2.0;
        }

        let basis = if dual {
            "A+B".to_string()
        } else {
            o.source.clone()
        };
        let polarity = if self.corr_ema < -0.15 { -1.0 } else { 1.0 };

        let mut samples: Vec<i32> = Vec::new();
        let mut corr_num = 0.0;
        let mut corr_da = 0.0;
        let mut corr_db = 0.0;
        let mut t = self.emitted_until + SAMPLE_DT;
        let mut last_emitted = self.emitted_until;
        while t <= horizon {
            let ya = o.sample_at(t);
            let fused: Option<f64> = if dual {
                let x = self.other.as_ref().unwrap();
                let yb = x.sample_at(self.fit.map_a_to_b(t));
                match (ya, yb) {
                    (Some(a), Some(b)) => {
                        let (a, b) = (a as f64, b as f64);
                        corr_num += a * b;
                        corr_da += a * a;
                        corr_db += b * b;
                        Some(0.5 * (a + polarity * b))
                    }
                    (Some(a), None) => Some(a as f64),
                    (None, Some(b)) => Some(polarity * b as f64),
                    (None, None) => None,
                }
            } else {
                ya.map(|v| v as f64)
            };
            if let Some(mv) = fused {
                samples.push((mv * 1000.0).round() as i32);
                last_emitted = t;
            } else if !samples.is_empty() {
                break; // hull: emitter det vi har, resten neste runde
            }
            t += SAMPLE_DT;
        }
        if samples.is_empty() {
            return Vec::new();
        }
        self.emitted_until = last_emitted;

        // Oppdater polaritets-korrelasjonen (EMA over chunk-korrelasjon).
        if dual && corr_da > 1e-9 && corr_db > 1e-9 {
            let c = corr_num / (corr_da.sqrt() * corr_db.sqrt());
            self.corr_ema = 0.9 * self.corr_ema + 0.1 * c;
        }

        let conf = if dual {
            let m = self.fit.match_rate();
            let r = if self.fit.residual_ms.is_nan() {
                0.0
            } else {
                (1.0 - (self.fit.residual_ms / 20.0)).clamp(0.0, 1.0)
            };
            (0.5 * m + 0.5 * r).clamp(0.0, 1.0)
        } else {
            0.35 // solo: ærlig lav konfidens - ingen fusjon
        };

        self.total_out += samples.len() as u64;
        let elapsed_s = last_emitted - self.out_t0.unwrap();
        vec![SynthChunk {
            samples_uv: samples,
            elapsed_s,
            basis,
            conf,
            offset_ms: self.fit.a * 1000.0,
            drift_ppm: (self.fit.b - 1.0) * 1e6,
            residual_ms: self.fit.residual_ms,
            total: self.total_out,
        }]
    }

    /// Statuslinje for replay/driftslogg.
    pub fn debug_line(&self) -> String {
        format!(
            "fit ready={} pairs={} offset={:.1}ms drift={:.1}ppm residual={:.2}ms matchrate={:.0}% corr={:.2}",
            self.fit.ready,
            self.fit.pairs.len(),
            self.fit.a * 1000.0,
            (self.fit.b - 1.0) * 1e6,
            self.fit.residual_ms,
            self.fit.match_rate() * 100.0,
            self.corr_ema,
        )
    }
}
