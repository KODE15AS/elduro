export interface Sample {
  /** milliseconds since recording start */
  t: number
  bpm: number
  /** R-R intervals in ms delivered with this packet */
  rr: number[]
}

export interface Metrics {
  packets: number
  durationS: number
  meanBpm: number
  minBpm: number
  maxBpm: number
  meanIntervalMs: number
  jitterMs: number
  maxGapMs: number
  drops: number
  rrCount: number
}

export type LaneStatus =
  | 'idle'
  | 'scanning'
  | 'connecting'
  | 'streaming'
  | 'done'
  | 'error'

export interface LaneData {
  status: LaneStatus
  detail: string
  device: string
  battery: number | null
  samples: Sample[]
  metrics: Metrics | null
}

export interface StatusExtra {
  device?: string
  battery?: number | null
}

export interface CaptureCallbacks {
  onstatus: (state: string, detail?: string, extra?: StatusExtra) => void
  onsample: (s: Sample) => void
}

/// Phase 2 raw stream frames pushed from the backend to the ECG view.
export interface EcgStreamMsg {
  t: 'ecg' | 'acc'
  source: string
  ts_device_ns: number
  ts_host_ns: number
  elapsed_ms?: number
  /** ECG: number[] microvolts. ACC: [x, y, z][] milli-g. */
  samples: number[] | number[][]
  total: number
  gaps?: number
}

