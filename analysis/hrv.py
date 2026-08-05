"""Derived HRV pipeline for elduro recordings.

Reads a raw recording (JSONL) and produces per-window HRV metrics with an
honest uncertainty band. Design decisions (see docs/handover/HANDOVER-2-to-3.md
section 6, and chat-3 discussion):

- Native H10 RR (from the HR service, ~1 ms resolution) is the PRIMARY signal.
- R-peaks derived from the raw 130 Hz ECG are the VERIFICATION layer.
- RMSSD is never suppressed for poor windows. Instead each window carries a
  point estimate PLUS a [lo, hi] band from a correction-sensitivity sweep
  (keep / drop / interpolate the flagged beats). The band widens where beats
  are uncertain; it collapses to the point where they are clean. Only a window
  with essentially no usable beats is reported as an explicit gap.

Supported input line shapes (auto-detected):
  {"s":"hr","ts_host_ns":..,"bpm":..,"rr_ms":[..]}       (paired capture)
  {"s":"ecg","ts_device_ns":..,"ts_host_ns":..,"uv":[..]} (paired + agent)
  {"s":"acc","mg":[[x,y,z],..]}                            (agent recording)
"""
from __future__ import annotations
import argparse
import json
import math
from dataclasses import dataclass, asdict

import numpy as np

ECG_FS = 130.0
ECG_UPSAMPLE = 500.0  # sub-sample R-peak timing; 130 Hz -> 7.7 ms is too coarse
DEFAULT_WINDOW_S = 30.0
DEFAULT_STEP_S = 5.0
ARTIFACT_REL = 0.20      # relative jump vs local median that flags a beat
ARTIFACT_ABS = (300.0, 2000.0)  # physiologically plausible RR bounds (ms)
DEGRADED_PCT = 5.0       # > this % corrected -> window is "degraded" (ranged)
MIN_BEATS = 5            # fewer usable beats -> "no_signal" gap


@dataclass
class Recording:
    native_rr_ms: np.ndarray
    native_rr_t: np.ndarray   # beat time (s) since session start
    ecg_uv: np.ndarray        # concatenated ECG samples (microvolt)
    ecg_start_s: float        # offset (s) of first ECG sample since session start
    acc_mag_g: np.ndarray     # accelerometer magnitude (g), may be empty
    acc_t: np.ndarray
    device: str = "unknown"


def load_recording(path: str) -> Recording:
    hr_lines: list[tuple[int, list[float]]] = []
    ecg: list[int] = []
    ecg_first_host = None
    acc_mag: list[float] = []
    acc_first_host = None
    device = "unknown"
    t0 = None
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            v = json.loads(line)
            if v.get("type") == "elduro-pmd-recording" or v.get("s") == "header":
                device = v.get("device", device)
                continue
            s = v.get("s")
            host = v.get("ts_host_ns")
            if host is not None:
                t0 = host if t0 is None else min(t0, host)
            if s == "hr":
                hr_lines.append((host, v.get("rr_ms") or []))
            elif s == "ecg":
                ecg.extend(v.get("uv") or [])
                if ecg_first_host is None:
                    ecg_first_host = host
            elif s == "acc":
                for xyz in (v.get("mg") or []):
                    acc_mag.append(math.sqrt(sum(c * c for c in xyz)))
                if acc_first_host is None:
                    acc_first_host = host
    if t0 is None:
        t0 = 0

    rr = []
    for _, rrs in hr_lines:
        rr.extend(rrs)
    rr = np.array(rr, dtype=float)
    if hr_lines and rr.size:
        nat_t0 = (hr_lines[0][0] - t0) / 1e9
        rr_t = nat_t0 + np.cumsum(rr) / 1000.0
    else:
        rr_t = np.array([])

    ecg_arr = np.array(ecg, dtype=float) / 1000.0  # mV
    ecg_start = ((ecg_first_host - t0) / 1e9) if ecg_first_host is not None else 0.0
    acc_arr = np.array(acc_mag, dtype=float) / 1000.0  # g
    acc_start = ((acc_first_host - t0) / 1e9) if acc_first_host is not None else 0.0
    acc_t = acc_start + np.arange(len(acc_arr)) / 200.0
    return Recording(rr, rr_t, ecg_arr, ecg_start, acc_arr, acc_t, device)


def rmssd(rr: np.ndarray) -> float:
    if len(rr) < 2:
        return float("nan")
    d = np.diff(rr)
    return float(np.sqrt(np.mean(d * d)))


def flag_artifacts(rr: np.ndarray, k: int = 5, rel: float = ARTIFACT_REL) -> np.ndarray:
    bad = (rr < ARTIFACT_ABS[0]) | (rr > ARTIFACT_ABS[1])
    for i in range(len(rr)):
        lo = max(0, i - k)
        hi = min(len(rr), i + k + 1)
        lm = np.median(rr[lo:hi])
        if lm > 0 and abs(rr[i] - lm) > rel * lm:
            bad[i] = True
    return bad


@dataclass
class HrvResult:
    point: float
    lo: float
    hi: float
    pct_corrected: float
    n_beats: int
    quality: str


def hrv_band(rr: np.ndarray) -> HrvResult:
    """RMSSD point (interpolated) + [lo, hi] over keep/drop/interpolate."""
    n = len(rr)
    if n < MIN_BEATS:
        return HrvResult(float("nan"), float("nan"), float("nan"), 0.0, n, "no_signal")
    bad = flag_artifacts(rr)
    good = np.where(~bad)[0]
    keep = rr
    drop = rr[~bad] if (~bad).sum() >= 2 else rr
    interp = rr.copy()
    if len(good) >= 2:
        interp = np.interp(np.arange(n), good, rr[good])
    vals = [rmssd(keep), rmssd(drop), rmssd(interp)]
    vals = [x for x in vals if not math.isnan(x)]
    point = rmssd(interp)
    pct = 100.0 * bad.sum() / n
    quality = "degraded" if pct > DEGRADED_PCT else "good"
    return HrvResult(point, min(vals), max(vals), pct, n, quality)


def derive_ecg_rr(ecg_mv: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Return (rr_ms, rr_times_s_within_ecg, rpeaks_idx_upsampled). Requires neurokit2."""
    import neurokit2 as nk
    up = nk.signal_resample(ecg_mv, sampling_rate=ECG_FS, desired_sampling_rate=ECG_UPSAMPLE,
                            method="interpolation")
    cleaned = nk.ecg_clean(up, sampling_rate=ECG_UPSAMPLE)
    _, info = nk.ecg_peaks(cleaned, sampling_rate=ECG_UPSAMPLE, method="neurokit")
    rpeaks = np.asarray(info["ECG_R_Peaks"])
    if len(rpeaks) < 2:
        return np.array([]), np.array([]), rpeaks
    rr = np.diff(rpeaks) / ECG_UPSAMPLE * 1000.0
    rr_t = rpeaks[1:] / ECG_UPSAMPLE
    return rr, rr_t, rpeaks


def rolling(rr: np.ndarray, rr_t: np.ndarray, acc_mag: np.ndarray, acc_t: np.ndarray,
            window_s: float, step_s: float) -> list[dict]:
    out = []
    if not len(rr_t):
        return out
    t = rr_t[0] + window_s
    end = rr_t[-1]
    while t <= end + 1e-9:
        m = (rr_t >= t - window_s) & (rr_t < t)
        seg = rr[m]
        res = hrv_band(seg)
        motion = float("nan")
        if len(acc_t):
            am = acc_mag[(acc_t >= t - window_s) & (acc_t < t)]
            if len(am):
                motion = float(np.std(am))
        d = asdict(res)
        d["t_center_s"] = round(float(t - window_s / 2), 2)
        d["motion_std_g"] = None if math.isnan(motion) else round(motion, 4)
        out.append(d)
        t += step_s
    return out


def summarize(rr: np.ndarray) -> dict:
    if not len(rr):
        return {"n_rr": 0}
    res = hrv_band(rr)
    return {
        "n_rr": int(len(rr)),
        "mean_hr_bpm": round(60000.0 / float(np.mean(rr)), 1),
        "sdnn_ms": round(float(np.std(rr, ddof=1)), 1) if len(rr) > 1 else None,
        "rmssd_point_ms": round(res.point, 1),
        "rmssd_band_ms": [round(res.lo, 1), round(res.hi, 1)],
        "pct_corrected": round(res.pct_corrected, 1),
        "quality": res.quality,
    }


def analyze(path: str, window_s: float, step_s: float) -> dict:
    rec = load_recording(path)
    result: dict = {
        "source_file": path,
        "device": rec.device,
        "params": {"window_s": window_s, "step_s": step_s,
                   "artifact_rel": ARTIFACT_REL, "degraded_pct": DEGRADED_PCT},
        "native": None,
        "ecg_derived": None,
        "windows": [],
    }
    if len(rec.native_rr_ms):
        result["native"] = summarize(rec.native_rr_ms)
        result["windows"] = rolling(rec.native_rr_ms, rec.native_rr_t,
                                    rec.acc_mag_g, rec.acc_t, window_s, step_s)
    if len(rec.ecg_uv) > int(5 * ECG_FS):
        try:
            rr_e, _, _ = derive_ecg_rr(rec.ecg_uv)
            if len(rr_e):
                result["ecg_derived"] = summarize(rr_e)
                if result["native"] is None:
                    rr_e_t = rec.ecg_start_s + np.cumsum(rr_e) / 1000.0
                    result["windows"] = rolling(rr_e, rr_e_t, rec.acc_mag_g, rec.acc_t,
                                                window_s, step_s)
        except Exception as e:  # neurokit missing or detection failure
            result["ecg_derived"] = {"error": str(e)}
    return result


def make_figure(path: str, out_fig: str, window_s: float, step_s: float) -> None:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    rec = load_recording(path)
    res = analyze(path, window_s, step_s)
    wins = res["windows"]
    fig, ax = plt.subplots(2, 1, figsize=(13, 6.5), height_ratios=[2, 2])
    if len(rec.native_rr_ms):
        ax[0].plot(rec.native_rr_t, rec.native_rr_ms, "-o", ms=3, color="#093",
                   label="native H10 RR")
    ax[0].set_title("RR tachogram"); ax[0].set_ylabel("RR (ms)"); ax[0].legend(fontsize=8)
    if wins:
        c = np.array([w["t_center_s"] for w in wins])
        pt = np.array([w["point"] for w in wins])
        lo = np.array([w["lo"] for w in wins])
        hi = np.array([w["hi"] for w in wins])
        ax[1].fill_between(c, lo, hi, color="#093", alpha=0.20, label="RMSSD band (correction sweep)")
        ax[1].plot(c, pt, "-o", ms=3, color="#093", label="RMSSD point")
    ax[1].set_title("Rolling RMSSD with uncertainty band")
    ax[1].set_ylabel("RMSSD (ms)"); ax[1].set_xlabel("seconds"); ax[1].legend(fontsize=8)
    plt.tight_layout(); plt.savefig(out_fig, dpi=110)


def build_view(path: str, window_s: float, step_s: float) -> dict:
    """Compact bundle for the frontend Rhythm/HRV view (offline-first)."""
    rec = load_recording(path)
    result = analyze(path, window_s, step_s)
    bundle: dict = {
        "device": rec.device,
        "generated_from": path,
        "params": result["params"],
        "session": {"native": result["native"], "ecg_derived": result["ecg_derived"]},
        "windows": result["windows"],
        "ecg": None,
        "rpeaks_s": [],
        "flagged_ecg_s": [],
        "tachogram": {"native": None, "ecg_derived": None},
    }
    if len(rec.native_rr_ms):
        bad = flag_artifacts(rec.native_rr_ms)
        bundle["tachogram"]["native"] = {
            "t": [round(float(x), 3) for x in rec.native_rr_t],
            "rr": [round(float(x), 1) for x in rec.native_rr_ms],
            "flagged": [bool(b) for b in bad],
        }
    if len(rec.ecg_uv) > int(5 * ECG_FS):
        try:
            rr_e, _, rpeaks = derive_ecg_rr(rec.ecg_uv)
            bundle["ecg"] = {
                "fs": ECG_FS,
                "start_s": round(rec.ecg_start_s, 3),
                "mv": [round(float(x), 3) for x in rec.ecg_uv],
            }
            rpeaks_s = rec.ecg_start_s + np.asarray(rpeaks) / ECG_UPSAMPLE
            bundle["rpeaks_s"] = [round(float(x), 3) for x in rpeaks_s]
            if len(rr_e):
                bad_e = flag_artifacts(rr_e)
                end_t = rpeaks_s[1:]
                bundle["flagged_ecg_s"] = [round(float(t), 3) for t, b in zip(end_t, bad_e) if b]
                bundle["tachogram"]["ecg_derived"] = {
                    "t": [round(float(x), 3) for x in end_t],
                    "rr": [round(float(x), 1) for x in rr_e],
                }
        except Exception as e:
            bundle["ecg"] = {"error": str(e)}
    return bundle


def main() -> None:
    ap = argparse.ArgumentParser(description="Derived HRV pipeline for elduro recordings.")
    ap.add_argument("recording", help="path to a JSONL recording")
    ap.add_argument("--out-json", help="write per-window HRV JSON here")
    ap.add_argument("--out-fig", help="write a PNG figure here")
    ap.add_argument("--out-view", help="write a compact bundle JSON for the frontend view here")
    ap.add_argument("--window", type=float, default=DEFAULT_WINDOW_S)
    ap.add_argument("--step", type=float, default=DEFAULT_STEP_S)
    args = ap.parse_args()

    if args.out_view:
        with open(args.out_view, "w") as fh:
            json.dump(build_view(args.recording, args.window, args.step), fh)
        print("wrote view bundle:", args.out_view)

    result = analyze(args.recording, args.window, args.step)
    if args.out_json:
        with open(args.out_json, "w") as fh:
            json.dump(result, fh, indent=2)
    else:
        summary = {k: result[k] for k in ("device", "native", "ecg_derived")}
        summary["n_windows"] = len(result["windows"])
        print(json.dumps(summary, indent=2))
    if args.out_fig:
        make_figure(args.recording, args.out_fig, args.window, args.step)


if __name__ == "__main__":
    main()
