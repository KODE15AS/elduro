import type { LaneData, Metrics, Sample } from './types'

export function emptyLane(): LaneData {
  return {
    status: 'idle',
    detail: '',
    device: '',
    battery: null,
    samples: [],
    metrics: null,
  }
}

export function computeMetrics(samples: Sample[]): Metrics | null {
  if (samples.length < 2) return null

  const intervals: number[] = []
  for (let i = 1; i < samples.length; i++) {
    intervals.push(samples[i].t - samples[i - 1].t)
  }
  const mean = intervals.reduce((a, b) => a + b, 0) / intervals.length
  const variance =
    intervals.reduce((a, b) => a + (b - mean) * (b - mean), 0) / intervals.length
  const sorted = [...intervals].sort((a, b) => a - b)
  const median = sorted[Math.floor(sorted.length / 2)]
  const maxGap = Math.max(...intervals)
  const drops = intervals.filter((i) => i > 1.8 * median).length

  const bpms = samples.map((s) => s.bpm).filter((b) => b > 0)
  const meanBpm = bpms.length ? bpms.reduce((a, b) => a + b, 0) / bpms.length : 0

  return {
    packets: samples.length,
    durationS: Math.round((samples[samples.length - 1].t - samples[0].t) / 100) / 10,
    meanBpm: Math.round(meanBpm),
    minBpm: bpms.length ? Math.min(...bpms) : 0,
    maxBpm: bpms.length ? Math.max(...bpms) : 0,
    meanIntervalMs: Math.round(mean),
    jitterMs: Math.round(Math.sqrt(variance)),
    maxGapMs: Math.round(maxGap),
    drops,
    rrCount: samples.reduce((a, s) => a + s.rr.length, 0),
  }
}

