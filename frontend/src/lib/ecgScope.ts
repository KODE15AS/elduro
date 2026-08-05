// Shared clinical-ECG scope engine.
//
// This is the single source of truth for "ECG generation" used by BOTH the RAW
// ECG tab and the RHYTHM/HRV tab. It owns the sample ingest (baseline removal),
// the R-peak detection + sticky classification, the noise/quality gate, the
// fixed-speed paper motor, and the grid + trace + peak drawing. Keeping it in
// one place guarantees the two tabs render an identical live ECG and makes the
// signal path maintainable from one file.
//
// It is a plain (non-reactive) class: components mirror the few display values
// they need (hrBpm, counters, freshness) into their own reactive state.

export interface EcgIngestMsg {
  t: 'ecg' | 'acc' | string
  elapsed_ms?: number
  ts_host_ns?: number
  ts_device_ns?: number
  samples: number[] | number[][]
  total?: number
  gaps?: number
}

export type PeakCls = 'pending' | 'norm' | 'ect'
export interface EcgPeak {
  t: number
  y: number
  cls: PeakCls
}

const ECG_FS = 130
const BUF_S = 60 // ring length in seconds
const LAG = 0.3 // tiny display buffer so the newest QRS isn't clipped mid-draw
const PX_PER_MM = 96 / 25.4 // ~96 dpi CSS millimetre; true nominal mm keeps mm/s honest
const MM_PER_MV = 10 // clinical gain 10 mm/mV
const BASE_TAU = 0.75 // s, causal baseline (wander) time constant
const PEAK_MARGIN = 0.3 // s of right context required before a sample is scanned
const BASE_ALPHA = 1 - Math.exp(-1 / (ECG_FS * BASE_TAU))

function median(a: number[]): number {
  if (!a.length) return 0
  const s = a.slice().sort((x, y) => x - y)
  return s[s.length >> 1]
}

export class EcgScope {
  readonly ECG_FS = ECG_FS
  readonly PX_PER_MM = PX_PER_MM
  readonly MM_PER_MV = MM_PER_MV
  readonly LAG = LAG

  private ecgCap = ECG_FS * BUF_S
  private ecgY = new Float32Array(this.ecgCap)
  private ecgT = new Float64Array(this.ecgCap)
  private ecgPoor = new Uint8Array(this.ecgCap) // 1 = signal too noisy to trust (frozen)
  private ecgHead = 0
  private ecgFilled = 0
  private ecgSeq = 0 // monotonic count of samples ever written
  private ecgLastT = -Infinity
  private baseEMA = 0
  private baseInit = false

  // detection state (sticky, committed once and never revised)
  peaks: EcgPeak[] = []
  private detSeq = 0
  lastPeakT = -Infinity
  private recentMax = 0 // decaying max R-wave amplitude -> adaptive threshold
  poorState = false
  private hiWinLoSeq = 0
  private hiCount = 0

  // ECG frames carry elapsed_ms; ACC frames only carry ts_host_ns. Map the host
  // (wall) clock onto the ECG elapsed clock so both share one timeline.
  hostElapsedOffset = NaN

  // fixed-speed paper motor: nowT advances at exactly wall-clock rate from an
  // anchor, re-anchored only on first data / resume / big desync.
  nowT = 0
  nowInit = false
  private dataAnchored = false
  private forceAnchor = false
  private perf0 = 0
  private nowBase = 0

  // display values the components mirror into reactive state
  ecgNewestT = 0
  hrBpm: number | null = null
  ecgTotal = 0
  gaps = 0
  deviceTimeS = 0
  lastEcgMs = 0

  reset(): void {
    this.ecgY = new Float32Array(this.ecgCap)
    this.ecgT = new Float64Array(this.ecgCap)
    this.ecgPoor = new Uint8Array(this.ecgCap)
    this.ecgHead = 0
    this.ecgFilled = 0
    this.ecgSeq = 0
    this.ecgLastT = -Infinity
    this.baseEMA = 0
    this.baseInit = false
    this.hostElapsedOffset = NaN
    this.peaks = []
    this.detSeq = 0
    this.lastPeakT = -Infinity
    this.recentMax = 0
    this.poorState = false
    this.hiWinLoSeq = 0
    this.hiCount = 0
    this.nowInit = false
    this.dataAnchored = false
    this.forceAnchor = false
    this.nowT = 0
    this.ecgNewestT = 0
    this.hrBpm = null
    this.ecgTotal = 0
    this.gaps = 0
    this.deviceTimeS = 0
  }

  // Ask the motor to snap to the newest data on the next tick (used on resume).
  requestAnchor(): void {
    this.forceAnchor = true
  }

  elapsedOf(m: EcgIngestMsg): number {
    if (typeof m.elapsed_ms === 'number') {
      if (m.ts_host_ns) this.hostElapsedOffset = m.elapsed_ms / 1000 - m.ts_host_ns / 1e9
      return m.elapsed_ms / 1000
    }
    if (m.ts_host_ns && !Number.isNaN(this.hostElapsedOffset))
      return m.ts_host_ns / 1e9 + this.hostElapsedOffset
    return m.ts_host_ns ? m.ts_host_ns / 1e9 : 0
  }

  // Anchor a packet's samples on the host clock. The newest sample sits at
  // elapsed_ms; earlier ones step back at the native rate so dropped packets
  // become real gaps. A forward-only shift keeps times strictly increasing.
  private ecgTimeBase(E: number, n: number): number {
    const base = E - (n - 1) / ECG_FS
    return this.ecgLastT > -Infinity
      ? Math.max(base, this.ecgLastT + 1 / ECG_FS - (n - 1) / ECG_FS)
      : base
  }

  private commitEcgSample(v: number, t: number): void {
    if (!this.baseInit) {
      this.baseEMA = v
      this.baseInit = true
    } else {
      // clamp the innovation so a movement spike cannot drag the baseline
      const innov = v - this.baseEMA
      this.baseEMA += BASE_ALPHA * Math.max(-0.4, Math.min(0.4, innov))
    }
    this.ecgY[this.ecgHead] = v - this.baseEMA // frozen display value
    this.ecgT[this.ecgHead] = t
    this.ecgHead = (this.ecgHead + 1) % this.ecgCap
    if (this.ecgFilled < this.ecgCap) this.ecgFilled++
    this.ecgSeq++
  }

  private seqAvail(seq: number): boolean {
    return seq >= this.ecgSeq - this.ecgFilled && seq >= 0 && seq < this.ecgSeq
  }
  private yAt(seq: number): number {
    return this.ecgY[((seq % this.ecgCap) + this.ecgCap) % this.ecgCap]
  }
  private tAt(seq: number): number {
    return this.ecgT[((seq % this.ecgCap) + this.ecgCap) % this.ecgCap]
  }
  private poorAt(seq: number): number {
    return this.ecgPoor[((seq % this.ecgCap) + this.ecgCap) % this.ecgCap]
  }

  private medRR(uptoIndex: number): number {
    const lo = Math.max(1, uptoIndex - 7)
    const rr: number[] = []
    for (let k = lo; k <= uptoIndex; k++)
      rr.push((this.peaks[k].t - this.peaks[k - 1].t) * 1000)
    return median(rr)
  }

  // Feed one 'ecg' stream frame. Baseline is removed at ingest and the value is
  // frozen; a printed sample never moves again.
  ingestEcg(m: EcgIngestMsg): void {
    const E = this.elapsedOf(m)
    const s = m.samples as number[]
    if (!s.length) return
    const base = this.ecgTimeBase(E, s.length)
    for (let j = 0; j < s.length; j++) this.commitEcgSample(s[j] / 1000, base + j / ECG_FS)
    this.ecgLastT = base + (s.length - 1) / ECG_FS
    this.ecgNewestT = this.ecgLastT
    if (typeof m.total === 'number') this.ecgTotal = m.total
    this.gaps = m.gaps ?? 0
    if (typeof m.ts_device_ns === 'number') this.deviceTimeS = m.ts_device_ns / 1e9
    this.lastEcgMs = performance.now()
  }

  // Detect + classify peaks up to (newest - PEAK_MARGIN). A rolling noise gate
  // marks stretches where the high-amplitude fraction is so large that no real
  // QRS can be trusted (movement) -> those are frozen "poor" and detection is
  // suppressed there, so we never print scattered false beats. Peaks are
  // committed once (pending) and classified once their successor exists.
  private runDetection(): void {
    const guardT = this.ecgNewestT - PEAK_MARGIN
    while (this.detSeq < this.ecgSeq) {
      if (this.detSeq < 2 || !this.seqAvail(this.detSeq - 2)) {
        if (this.seqAvail(this.detSeq))
          this.ecgPoor[((this.detSeq % this.ecgCap) + this.ecgCap) % this.ecgCap] = this.poorState ? 1 : 0
        this.detSeq++
        continue
      }
      if (this.detSeq + 2 >= this.ecgSeq || !this.seqAvail(this.detSeq + 2)) break
      const s = this.detSeq
      const ti = this.tAt(s)
      if (ti > guardT) break
      const yi = this.yAt(s)
      const ayi = Math.abs(yi)
      // rolling high-amplitude fraction over a trailing ~1.5 s window
      if (ayi > 0.35) this.hiCount++
      while (this.hiWinLoSeq < s && ti - this.tAt(this.hiWinLoSeq) > 1.5) {
        if (Math.abs(this.yAt(this.hiWinLoSeq)) > 0.35) this.hiCount--
        this.hiWinLoSeq++
      }
      const hiFrac = this.hiCount / Math.max(1, s - this.hiWinLoSeq + 1)
      if (!this.poorState) {
        if (hiFrac > 0.4) this.poorState = true
      } else {
        if (hiFrac < 0.25) this.poorState = false
      }
      this.ecgPoor[((s % this.ecgCap) + this.ecgCap) % this.ecgCap] = this.poorState ? 1 : 0
      if (!this.poorState) {
        // adaptive threshold: bootstraps from the signal itself so detection
        // starts on the first real R-wave (no shaking needed)
        this.recentMax = Math.max(yi, this.recentMax * 0.999)
        const thr = Math.max(0.18, 0.45 * this.recentMax)
        const isMax =
          yi >= thr &&
          yi >= this.yAt(s - 1) && yi >= this.yAt(s - 2) &&
          yi >= this.yAt(s + 1) && yi >= this.yAt(s + 2)
        if (isMax && ti - this.lastPeakT > 0.3) {
          this.peaks.push({ t: ti, y: yi, cls: 'pending' })
          this.lastPeakT = ti
        }
      }
      this.detSeq++
    }
    // classify any pending peak that now has a successor
    for (let i = 0; i < this.peaks.length - 1; i++) {
      if (this.peaks[i].cls !== 'pending') continue
      const med = this.medRR(i + 1)
      const prev = i > 0 ? (this.peaks[i].t - this.peaks[i - 1].t) * 1000 : med
      const next = (this.peaks[i + 1].t - this.peaks[i].t) * 1000
      const ectopic = prev < 0.8 * med || next < 0.8 * med || prev > 1.6 * med || next > 1.6 * med
      this.peaks[i].cls = ectopic ? 'ect' : 'norm'
      if (med > 250 && med < 2000) this.hrBpm = Math.round(60000 / med)
    }
    if (this.peaks.length > 500) this.peaks = this.peaks.slice(-400)
  }

  // Advance the paper motor one frame. `live` = recording and not paused.
  tick(perf: number, live: boolean): void {
    if (!live) return
    if (!this.nowInit) {
      this.perf0 = perf
      this.nowBase = 0
      this.nowInit = true
      this.dataAnchored = false
    }
    if (perf - this.lastEcgMs > 1500) {
      // Data stalled: freeze the paper instead of running forward and snapping
      // back every ~3 s (which reads as a repeating loop of the last beats).
      this.perf0 = perf
      this.nowBase = this.nowT
    } else {
      this.nowT = this.nowBase + (perf - this.perf0) / 1000
      if (this.ecgNewestT > 0) {
        const err = this.ecgNewestT + LAG - this.nowT
        // anchor to data on first packet, on resume, or after a big desync
        if (!this.dataAnchored || this.forceAnchor || Math.abs(err) > 3) {
          this.perf0 = perf
          this.nowBase = this.ecgNewestT + LAG
          this.nowT = this.nowBase
          this.dataAnchored = true
          this.forceAnchor = false
        }
        this.runDetection()
        if (this.nowT - this.lastPeakT > 2.5) this.hrBpm = null
      }
    }
  }

  // Clinical grid at nominal CSS millimetres, anchored to absolute time so the
  // paper scrolls continuously. 5 mm major box = 0.2 s at 25 mm/s (0.1 s at 50);
  // vertical gain fixed at 10 mm/mV. Static so the HRV sample-mode strip can draw
  // the exact same paper without a live engine instance.
  static drawPaperGrid(
    ctx: CanvasRenderingContext2D, w: number, h: number,
    t0: number, xFor: (t: number) => number, centerY: number, spd: number,
  ): void {
    const minorS = 1 / spd
    const majorS = 5 / spd
    const mmPx = PX_PER_MM
    ctx.lineWidth = 1
    ctx.strokeStyle = '#f6dede'
    for (let t = Math.ceil(t0 / minorS) * minorS; xFor(t) <= w; t += minorS) {
      const x = xFor(t); ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke()
    }
    for (let mm = 0; centerY - mm * mmPx > 0 || centerY + mm * mmPx < h; mm += 1) {
      const yUp = centerY - mm * mmPx, yDn = centerY + mm * mmPx
      if (yUp > 0) { ctx.beginPath(); ctx.moveTo(0, yUp); ctx.lineTo(w, yUp); ctx.stroke() }
      if (mm && yDn < h) { ctx.beginPath(); ctx.moveTo(0, yDn); ctx.lineTo(w, yDn); ctx.stroke() }
    }
    ctx.strokeStyle = '#dd9090'
    ctx.lineWidth = 1.2
    for (let t = Math.ceil(t0 / majorS) * majorS; xFor(t) <= w; t += majorS) {
      const x = xFor(t); ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke()
    }
    for (let mm = 0; centerY - mm * mmPx > 0 || centerY + mm * mmPx < h; mm += 5) {
      const yUp = centerY - mm * mmPx, yDn = centerY + mm * mmPx
      if (yUp > 0) { ctx.beginPath(); ctx.moveTo(0, yUp); ctx.lineTo(w, yUp); ctx.stroke() }
      if (mm && yDn < h) { ctx.beginPath(); ctx.moveTo(0, yDn); ctx.lineTo(w, yDn); ctx.stroke() }
    }
  }

  // Draw grid + frozen trace + sticky peak markers for the given paper speed.
  // Returns the number of samples drawn so the caller can decide whether to show
  // a warmup overlay. Poor (noisy) stretches render as a flat grey line.
  drawScope(ctx: CanvasRenderingContext2D, w: number, h: number, speed: number): number {
    const winS = w / PX_PER_MM / speed
    const t1 = this.nowInit ? this.nowT : winS
    const t0 = t1 - winS
    const xFor = (t: number) => (t - t0) * speed * PX_PER_MM
    const centerY = h / 2
    const yFor = (y: number) => centerY - y * MM_PER_MV * PX_PER_MM

    EcgScope.drawPaperGrid(ctx, w, h, t0, xFor, centerY, speed)

    const gapTol = 1.8 / ECG_FS
    ctx.lineJoin = 'round'
    let seg = -1 // -1 none, 0 good, 1 poor
    let prevT = -Infinity
    let open = false
    let drew = 0
    const stroke = () => { if (open) { ctx.stroke(); open = false } }
    const startSeq = Math.max(this.ecgSeq - this.ecgFilled, 0)
    for (let seq = startSeq; seq < this.ecgSeq; seq++) {
      const t = this.tAt(seq)
      if (t < t0 - 0.2) continue
      if (t > t1) break
      const poor = this.poorAt(seq)
      const y = poor ? centerY : yFor(this.yAt(seq))
      const x = xFor(t)
      if (poor !== seg || t - prevT > gapTol) {
        stroke()
        ctx.beginPath()
        ctx.strokeStyle = poor ? '#c4beb2' : '#111'
        ctx.lineWidth = poor ? 2 : 1.3
        ctx.moveTo(x, y)
        open = true
        seg = poor
      } else {
        ctx.lineTo(x, y)
      }
      prevT = t
      drew++
    }
    stroke()

    // frozen, sticky peak markers (only classified beats -> one beat behind)
    for (const pk of this.peaks) {
      if (pk.cls === 'pending' || pk.t < t0 || pk.t > t1) continue
      ctx.fillStyle = pk.cls === 'ect' ? '#d98a00' : '#cb333b'
      const yy = yFor(pk.y) - 8
      ctx.beginPath(); ctx.arc(xFor(pk.t), yy, pk.cls === 'ect' ? 5 : 4, 0, Math.PI * 2); ctx.fill()
    }
    return drew
  }
}

