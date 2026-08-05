# elduro analysis - derived HRV pipeline

Regenerates HRV metrics from raw recordings. Nothing here mutates raw data; it
reads a recording and emits derived output (per handover 2->3, two-tier model).

## Method (decided in chat 3)

- **Native H10 RR is the primary signal** (HR service, ~1 ms resolution, Polar's
  validated on-strap detector). **ECG-derived R-peaks are the verification
  layer** - used to check whether a suspicious interval is a real beat (present
  in both) or a detection artifact (present in one).
- **RMSSD is never hidden for poor windows.** Each window carries a point
  estimate plus a `[lo, hi]` band from a correction-sensitivity sweep
  (keep / drop / interpolate the flagged beats). The band widens where beats are
  uncertain and collapses where they are clean. Only windows with essentially no
  usable beats (`n_beats < 5`) are reported as an explicit `no_signal` gap.
- Window quality: `good` (<=5% corrected), `degraded` (>5%, shown as a wide
  band), `no_signal`.
- Artifact/ectopic handling and RMSSD sensitivity follow the Task Force (1996)
  and Kubios cautions summarized in the handover.

## Usage

```sh
python hrv.py <recording.jsonl> --out-json out.json --out-fig fig.png
# defaults: --window 30 --step 5 (ultra-short rolling RMSSD)
```

Accepts both the paired capture (`s:"hr"` + `s:"ecg"`) and the agent PMD
recording (`s:"ecg"` + `s:"acc"`).

## Output (JSON)

`native` and `ecg_derived` session summaries (mean HR, SDNN, RMSSD point + band,
% corrected, quality) plus a `windows` array of rolling RMSSD with band, %
corrected, beat count, quality and motion (ACC std) per window.

## Install

```sh
pip install -r requirements.txt
```

Status: exploratory instrument. There is no established RMSSD<->vagal<->
myocardial-bridge relationship for this subject; output is for personal
longitudinal exploration, not diagnosis.
