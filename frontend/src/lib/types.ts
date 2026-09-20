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
