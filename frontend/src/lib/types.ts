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

