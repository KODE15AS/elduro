<script lang="ts">
  import { onMount } from 'svelte'
  import { EcgScope } from './ecgScope'

  interface Props {
    src?: string
    sources?: Record<string, string>
    send?: (cmd: object) => void
    register?: (fn: (m: any) => void) => void
    onstatus?: (source: string) => { state: string; detail: string; device: string } | null
  }
  let {
    src = '/sample-hrv.json',
    sources = {},
    send,
    register,
    onstatus,
  }: Props = $props()

  const STRIP_WIN = 6 // seconds shown in the rhythm strip (sample mode)
  const TACHO_WIN = 20 // tachogram scroll window (right->left), roughly follows the strip
  const RMSSD_VIEW = 120 // scrolling RMSSD span in live mode
  const RMSSD_HALF = 15 // centered RMSSD window half-width -> 30 s window
  const RMSSD_STEP = 2
  // Fixed axes for the live plots: an outlier clips off the plot instead of
  // rescaling everything on a single bad value.
  const RR_LO = 400, RR_HI = 1200 // tachogram y-axis (ms)
  const RMSSD_HI = 150 // RMSSD y-axis (ms)
  const PX_PER_MM = 96 / 25.4 // ~96 dpi CSS millimetre, matches RAW ECG
  const MM_PER_MV = 10 // clinical gain 10 mm/mV, matches RAW ECG
  const SPEEDS = [25, 50] // mm/s selectable, clinical paper speed for the strip

  // Shared clinical-ECG engine: the RAW ECG tab runs this same code, so the live
  // rhythm strip's signal path (baseline, detection, motor, drawing) is identical.
  const scope = new EcgScope()

  let mode = $state<'sample' | 'live'>('sample')
  let bundle: any = $state(null)
  let error = $state('')
  let stripStart = $state(0)

  // live capture state
  let selected = $state('')
  let wantLive = $state(false)
  let lastRr = $state<number | null>(null)
  let instHr = $state<number | null>(null)
  const sourceIds = $derived(Object.keys(sources))
  const liveStatus = $derived(selected && onstatus ? onstatus(selected) : null)
  let liveFresh = $state(false)
  let hrFresh = $state(false)
  // The button reflects the user's intent only; a foreign raw-ECG stream can set
  // liveFresh, but that must not hide GO LIVE (which starts an HRV session).
  const running = $derived(wantLive)

  let stripCanvas: HTMLCanvasElement
  let tachoCanvas: HTMLCanvasElement
  let rmssdCanvas: HTMLCanvasElement

  // ---- live native-RR accumulators (ECG lives in `scope`) ----
  let rrVals: number[] = []
  let rrTimes: number[] = []
  let lastHrMs = 0

  let speed = $state(25) // mm/s clinical paper speed for the strip
  let paused = $state(false)
  // throttled RMSSD windows
  let liveWins: any[] = []
  let lastWinMs = 0

  function resetLive() {
    scope.reset()
    rrVals = []
    rrTimes = []
    liveWins = []
    lastRr = null
    instHr = null
  }

  function preferHrvSource(ids: string[]): string {
    return (
      ids.find((i) => i.startsWith('raven:hci1')) ??
      ids.find((i) => !i.startsWith('raven:hci0')) ??
      ids[0] ??
      ''
    )
  }
  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    return `native agent (${id.split(':')[0]})`
  }

  $effect(() => {
    if (!selected && sourceIds.length) selected = preferHrvSource(sourceIds)
  })

  async function loadSample() {
    try {
      const r = await fetch(src)
      if (!r.ok) throw new Error(`fetch ${src}: ${r.status}`)
      bundle = await r.json()
    } catch (e) {
      error = String(e)
    }
  }

  function onFrame(m: any) {
    if (mode !== 'live' || m.source !== selected) return
    if (m.t === 'ecg') {
      // Same engine as RAW ECG: baseline removal + sticky detection happen inside.
      scope.ingestEcg(m)
    } else if (m.t === 'hr') {
      lastHrMs = performance.now()
      const rr = (m.rr as number[]) ?? []
      if (!rr.length) return
      const tEnd = (m.ts as number) / 1000
      let acc = 0
      const tail = rr.map((v) => (acc += v))
      const total = acc
      // Anchor the packet's last beat near tEnd but force the beat clock to be
      // strictly increasing, so a jittered packet can never place a point to
      // the left of an earlier one (was the "points go back in time" bug).
      let prev = rrTimes.length ? rrTimes[rrTimes.length - 1] : -Infinity
      rr.forEach((v, i) => {
        let t = tEnd - (total - tail[i]) / 1000
        if (t <= prev) t = prev + 0.002
        prev = t
        rrVals.push(v)
        rrTimes.push(t)
      })
      lastRr = rr[rr.length - 1]
      instHr = Math.round(60000 / lastRr)
      const cap = 2400
      if (rrVals.length > cap) {
        rrVals = rrVals.slice(-cap)
        rrTimes = rrTimes.slice(-cap)
      }
    }
  }

  // ---------- HRV math (mirrors analysis/hrv.py) ----------
  function flagArtifacts(rr: number[]): boolean[] {
    const bad = rr.map((v) => v < 300 || v > 2000)
    const k = 5
    for (let i = 0; i < rr.length; i++) {
      const lo = Math.max(0, i - k)
      const hi = Math.min(rr.length, i + k + 1)
      const seg = rr.slice(lo, hi).slice().sort((a, b) => a - b)
      const med = seg[Math.floor(seg.length / 2)]
      if (med > 0 && Math.abs(rr[i] - med) > 0.2 * med) bad[i] = true
    }
    return bad
  }
  function rmssdOf(rr: number[]): number {
    if (rr.length < 2) return NaN
    let s = 0
    for (let i = 1; i < rr.length; i++) {
      const d = rr[i] - rr[i - 1]
      s += d * d
    }
    return Math.sqrt(s / (rr.length - 1))
  }
  function interpFlagged(rr: number[], bad: boolean[]): number[] {
    const gi: number[] = []
    const gv: number[] = []
    rr.forEach((v, i) => {
      if (!bad[i]) {
        gi.push(i)
        gv.push(v)
      }
    })
    if (gi.length < 2) return rr.slice()
    const out = rr.slice()
    for (let i = 0; i < rr.length; i++) {
      if (!bad[i]) continue
      if (i <= gi[0]) out[i] = gv[0]
      else if (i >= gi[gi.length - 1]) out[i] = gv[gv.length - 1]
      else {
        let k = 0
        while (k < gi.length - 1 && gi[k + 1] < i) k++
        const x0 = gi[k], x1 = gi[k + 1], y0 = gv[k], y1 = gv[k + 1]
        out[i] = y0 + (y1 - y0) * ((i - x0) / (x1 - x0))
      }
    }
    return out
  }
  function sdnnOf(rr: number[]): number {
    if (rr.length < 2) return NaN
    const m = rr.reduce((a, b) => a + b, 0) / rr.length
    let s = 0
    for (const v of rr) s += (v - m) * (v - m)
    return Math.sqrt(s / (rr.length - 1))
  }
  function bandOf(rr: number[]) {
    const n = rr.length
    if (n < 5)
      return { point: NaN, lo: NaN, hi: NaN, pct: 0, quality: 'no_signal', n, mean_hr: NaN, sdnn: NaN }
    const bad = flagArtifacts(rr)
    const nbad = bad.filter(Boolean).length
    const good = rr.filter((_, i) => !bad[i])
    const drop = good.length >= 2 ? good : rr
    const interp = interpFlagged(rr, bad)
    const cand = [rmssdOf(rr), rmssdOf(drop), rmssdOf(interp)].filter((x) => !isNaN(x))
    const pct = (100 * nbad) / n
    const meanRR = interp.reduce((a, b) => a + b, 0) / interp.length
    const r1 = (x: number) => Math.round(x * 10) / 10
    return {
      point: r1(rmssdOf(interp)),
      lo: r1(Math.min(...cand)),
      hi: r1(Math.max(...cand)),
      pct: r1(pct),
      quality: pct > 5 ? 'degraded' : 'good',
      n,
      mean_hr: Math.round(60000 / meanRR),
      sdnn: r1(sdnnOf(interp)),
    }
  }

  function computeLiveWindows(times: number[], rr: number[], nowT: number) {
    const out: any[] = []
    const startC = Math.max(RMSSD_HALF, nowT - RMSSD_VIEW - RMSSD_HALF)
    for (let c = startC; c <= nowT + RMSSD_STEP; c += RMSSD_STEP) {
      const lo = c - RMSSD_HALF, hi = c + RMSSD_HALF
      const idx: number[] = []
      for (let i = 0; i < times.length; i++) if (times[i] >= lo && times[i] < hi) idx.push(i)
      const provisional = c + RMSSD_HALF > nowT
      if (idx.length < 5) {
        out.push({ t_center_s: c, point: NaN, lo: NaN, hi: NaN, quality: 'no_signal', pct_corrected: 0, n_beats: idx.length, provisional })
        continue
      }
      const b = bandOf(idx.map((i) => rr[i]))
      const N = idx.length
      const se = b.point / Math.sqrt(2 * Math.max(1, N - 1))
      out.push({
        t_center_s: c,
        point: b.point,
        lo: Math.max(0, Math.min(b.lo, b.point - 1.96 * se)),
        hi: Math.max(b.hi, b.point + 1.96 * se),
        quality: b.quality,
        pct_corrected: b.pct,
        n_beats: N,
        provisional,
      })
    }
    return out
  }

  function buildLiveBundle() {
    const rr = rrVals.slice()
    const times = rrTimes.slice()
    const trailIdx: number[] = []
    for (let i = 0; i < times.length; i++) if (times[i] >= scope.ecgNewestT - 30) trailIdx.push(i)
    const sess = bandOf(trailIdx.map((i) => rr[i]))
    // ECG-derived RR from the shared engine's sticky peaks, so it matches the
    // strip markers exactly and stops flickering.
    const pk = scope.peaks
    const ecgRrT: number[] = []
    const ecgRrV: number[] = []
    for (let i = 1; i < pk.length; i++) {
      if (pk[i - 1].t < scope.ecgNewestT - (TACHO_WIN + 6)) continue
      ecgRrT.push(pk[i].t)
      ecgRrV.push((pk[i].t - pk[i - 1].t) * 1000)
    }
    return {
      ecgReady: scope.ecgNewestT > 0,
      ecgRr: { t: ecgRrT, v: ecgRrV },
      windows: liveWins,
      session: {
        native: {
          rmssd_point_ms: isNaN(sess.point) ? null : sess.point,
          rmssd_band_ms: [sess.lo, sess.hi],
          sdnn_ms: isNaN(sess.sdnn) ? null : sess.sdnn,
          pct_corrected: sess.pct,
        },
      },
      _axisRight: scope.nowT, // right edge = now (fixed motor)
      _lead: scope.nowT,
    }
  }

  function tMax(): number {
    if (!bundle) return 1
    let m = 1
    const n = bundle.tachogram?.native
    const e = bundle.tachogram?.ecg_derived
    if (n?.t?.length) m = Math.max(m, n.t[n.t.length - 1])
    if (e?.t?.length) m = Math.max(m, e.t[e.t.length - 1])
    if (bundle.ecg?.mv?.length) m = Math.max(m, bundle.ecg.start_s + bundle.ecg.mv.length / bundle.ecg.fs)
    return m
  }

  function fit(canvas: HTMLCanvasElement): [CanvasRenderingContext2D, number, number] | null {
    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (!w || !h) return null
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return null
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)
    return [ctx, w, h]
  }

  const QCOLOR: Record<string, string> = {
    good: '#0a9a4a',
    degraded: '#d98a00',
    no_signal: '#b0b0b0',
  }

  function drawStrip() {
    if (!stripCanvas) return
    // LIVE: the shared clinical-ECG engine draws grid + trace + sticky peaks,
    // identical to the RAW ECG tab.
    if (mode === 'live') {
      const f = fit(stripCanvas)
      if (!f) return
      const [ctx, w, h] = f
      scope.drawScope(ctx, w, h, speed)
      return
    }

    // SAMPLE: offline bundle viewer (static recording), drawn on the same paper.
    if (!bundle?.ecg?.mv) return
    const f = fit(stripCanvas)
    if (!f) return
    const [ctx, w, h] = f
    const fs = bundle.ecg.fs
    const mv = bundle.ecg.mv
    const start_s = bundle.ecg.start_s
    const t1 = stripStart + STRIP_WIN
    const t0 = t1 - STRIP_WIN
    const leadT = t1
    const centerY = h / 2
    const spd = w / PX_PER_MM / STRIP_WIN
    const xFor = (t: number) => (t - t0) * spd * PX_PER_MM
    const yFor = (v: number) => centerY - v * MM_PER_MV * PX_PER_MM

    EcgScope.drawPaperGrid(ctx, w, h, t0, xFor, centerY, spd)

    ctx.strokeStyle = '#111'
    ctx.lineWidth = 1.2
    ctx.beginPath()
    const iStart = Math.max(0, Math.floor((t0 - start_s) * fs))
    const iEnd = Math.min(mv.length, Math.ceil((leadT - start_s) * fs))
    let started = false
    for (let i = iStart; i < iEnd; i++) {
      const x = xFor(start_s + i / fs), y = yFor(mv[i])
      if (!started) { ctx.moveTo(x, y); started = true } else { ctx.lineTo(x, y) }
    }
    ctx.stroke()

    const mvAt = (t: number) => mv[Math.round((t - start_s) * fs)] ?? 0
    const peaks = (bundle.rpeaks_s ?? []).filter((rp: number) => rp >= t0 && rp <= leadT)
    const flagged = new Set((bundle.flagged_ecg_s ?? []).map((x: number) => Math.round(x * 1000)))
    for (const rp of peaks) {
      const isBad = flagged.has(Math.round(rp * 1000))
      ctx.fillStyle = isBad ? '#d98a00' : '#cb333b'
      ctx.beginPath(); ctx.arc(xFor(rp), yFor(mvAt(rp)) - 8, 4, 0, Math.PI * 2); ctx.fill()
    }
  }

  function drawTacho() {
    if (!tachoCanvas) return
    const f = fit(tachoCanvas)
    if (!f) return
    const [ctx, w, h] = f
    const live = mode === 'live'

    if (!live) {
      drawTachoSample(ctx, w, h)
      return
    }

    // LIVE: a clean textbook tachogram that scrolls right->left. The right edge
    // is "now"; the window matches the strip so the RR trend lines up with it.
    // Fixed y-axis - outliers clip off the plot. Two curves show whether the RR
    // sources agree: native H10 beats (solid green, reliable) vs the shared
    // in-browser ECG detector (dashed red).
    const t1 = bundle._axisRight
    const winS = w / PX_PER_MM / speed
    const t0 = t1 - winS
    const xFor = (t: number) => ((t - t0) / winS) * w
    const yFor = (rr: number) => h - ((rr - RR_LO) / (RR_HI - RR_LO)) * h

    ctx.fillStyle = '#bbb'
    ctx.font = '10px sans-serif'
    for (let r = RR_LO; r <= RR_HI; r += 100) {
      const y = yFor(r)
      ctx.strokeStyle = r % 200 === 0 ? '#e4e4e4' : '#f3f3f3'
      ctx.lineWidth = 1
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke()
      if (r % 200 === 0) ctx.fillText(String(r), 3, y - 2)
    }
    ctx.strokeStyle = '#f3f3f3'
    for (let t = Math.ceil(t0); t < t1; t += 1) {
      const x = xFor(t)
      ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke()
    }

    const er = bundle.ecgRr
    if (er?.t?.length) {
      ctx.setLineDash([4, 3])
      ctx.strokeStyle = 'rgba(203,51,59,0.55)'
      ctx.lineWidth = 1.2
      ctx.beginPath()
      let started = false
      for (let i = 0; i < er.t.length; i++) {
        if (er.t[i] < t0 || er.t[i] > t1) continue
        const x = xFor(er.t[i]), y = yFor(er.v[i])
        started ? ctx.lineTo(x, y) : ctx.moveTo(x, y)
        started = true
      }
      ctx.stroke()
      ctx.setLineDash([])
      ctx.fillStyle = 'rgba(203,51,59,0.7)'
      for (let i = 0; i < er.t.length; i++) {
        if (er.t[i] < t0 || er.t[i] > t1) continue
        ctx.beginPath(); ctx.arc(xFor(er.t[i]), yFor(er.v[i]), 2, 0, Math.PI * 2); ctx.fill()
      }
    }

    const idx: number[] = []
    for (let i = 0; i < rrTimes.length; i++) {
      if (rrTimes[i] >= t0 && rrTimes[i] <= t1) idx.push(i)
    }
    if (idx.length) {
      const bad = flagArtifacts(idx.map((i) => rrVals[i]))
      ctx.strokeStyle = '#0a9a4a'
      ctx.lineWidth = 1.8
      ctx.beginPath()
      idx.forEach((i, k) => {
        const x = xFor(rrTimes[i]), y = yFor(rrVals[i])
        k ? ctx.lineTo(x, y) : ctx.moveTo(x, y)
      })
      ctx.stroke()
      idx.forEach((i, k) => {
        ctx.fillStyle = bad[k] ? '#d98a00' : '#0a9a4a'
        ctx.beginPath()
        ctx.arc(xFor(rrTimes[i]), yFor(rrVals[i]), bad[k] ? 4 : 3, 0, Math.PI * 2)
        ctx.fill()
      })
    }
    ctx.fillStyle = '#777'
    ctx.font = '11px sans-serif'
    ctx.fillText(`RR (ms) - native H10 (green), ECG-derived (red dashed) - ${winS.toFixed(0)}s @ ${speed} mm/s`, 30, 14)
  }

  function drawTachoSample(ctx: CanvasRenderingContext2D, w: number, h: number) {
    if (!bundle?.tachogram) return
    const t1 = tMax()
    const nat = bundle.tachogram.native
    const ecg = bundle.tachogram.ecg_derived
    let lo = 400, hi = 1400
    for (const s of [nat, ecg]) {
      if (s?.rr?.length) { lo = Math.min(lo, ...s.rr); hi = Math.max(hi, ...s.rr) }
    }
    const pad = (hi - lo) * 0.1 || 50
    lo -= pad; hi += pad
    const xFor = (t: number) => (t / t1) * w
    const yFor = (rr: number) => h - ((rr - lo) / (hi - lo)) * h
    ctx.fillStyle = 'rgba(60,120,200,0.10)'
    ctx.fillRect(xFor(stripStart), 0, xFor(stripStart + STRIP_WIN) - xFor(stripStart), h)
    if (ecg?.t?.length) {
      ctx.strokeStyle = 'rgba(203,51,59,0.55)'; ctx.lineWidth = 1; ctx.beginPath()
      ecg.t.forEach((t: number, i: number) => { const x = xFor(t), y = yFor(ecg.rr[i]); i ? ctx.lineTo(x, y) : ctx.moveTo(x, y) })
      ctx.stroke()
    }
    if (nat?.t?.length) {
      ctx.strokeStyle = '#0a9a4a'; ctx.lineWidth = 1.4; ctx.beginPath()
      nat.t.forEach((t: number, i: number) => { const x = xFor(t), y = yFor(nat.rr[i]); i ? ctx.lineTo(x, y) : ctx.moveTo(x, y) })
      ctx.stroke()
      nat.t.forEach((t: number, i: number) => {
        const bad = nat.flagged?.[i]
        ctx.fillStyle = bad ? '#d98a00' : '#0a9a4a'
        ctx.beginPath(); ctx.arc(xFor(t), yFor(nat.rr[i]), bad ? 3.5 : 2, 0, Math.PI * 2); ctx.fill()
      })
    }
    ctx.fillStyle = '#777'; ctx.font = '11px sans-serif'
    ctx.fillText('RR (ms) - native (green), ECG-derived (red), flagged (amber)', 6, 14)
  }

  function drawRmssd() {
    if (!rmssdCanvas || !bundle?.windows) return
    const f = fit(rmssdCanvas)
    if (!f) return
    const [ctx, w, h] = f
    const live = mode === 'live'
    const t1 = live ? bundle._axisRight : tMax()
    const t0 = live ? t1 - RMSSD_VIEW : 0
    const leadT = live ? bundle._lead : t1
    const wins = bundle.windows.filter(
      (x: any) => x.quality !== 'no_signal' && !isNaN(x.point) &&
        (!live || (x.t_center_s >= t0 && x.t_center_s <= leadT)),
    )
    const HI = RMSSD_HI
    const xFor = (t: number) => ((t - t0) / (t1 - t0)) * w
    const yFor = (v: number) => h - (v / HI) * h

    ctx.fillStyle = '#bbb'
    ctx.font = '10px sans-serif'
    for (let r = 0; r <= HI; r += 50) {
      const y = yFor(r)
      ctx.strokeStyle = '#eee'; ctx.lineWidth = 1
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke()
      ctx.fillText(String(r), 3, y - 2)
    }
    const sdnn = bundle.session?.native?.sdnn_ms
    if (typeof sdnn === 'number' && sdnn > 0 && sdnn < HI) {
      const y = yFor(sdnn)
      ctx.setLineDash([5, 4]); ctx.strokeStyle = 'rgba(60,60,60,0.35)'; ctx.lineWidth = 1
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke()
      ctx.setLineDash([])
      ctx.fillStyle = 'rgba(60,60,60,0.75)'
      ctx.fillText(`SDNN ${sdnn.toFixed(0)}`, w - 62, y - 3)
    }

    for (let i = 1; i < wins.length; i++) {
      const a = wins[i - 1], b = wins[i]
      const prov = a.provisional || b.provisional
      ctx.fillStyle = prov ? 'rgba(150,150,150,0.30)' : 'rgba(10,154,74,0.18)'
      ctx.beginPath()
      ctx.moveTo(xFor(a.t_center_s), yFor(a.hi))
      ctx.lineTo(xFor(b.t_center_s), yFor(b.hi))
      ctx.lineTo(xFor(b.t_center_s), yFor(b.lo))
      ctx.lineTo(xFor(a.t_center_s), yFor(a.lo))
      ctx.closePath(); ctx.fill()
    }
    ctx.lineWidth = 1.6
    for (let i = 1; i < wins.length; i++) {
      const prov = wins[i].provisional || wins[i - 1].provisional
      ctx.strokeStyle = prov ? '#9a9a9a' : (QCOLOR[wins[i].quality] ?? '#0a9a4a')
      ctx.beginPath()
      ctx.moveTo(xFor(wins[i - 1].t_center_s), yFor(wins[i - 1].point))
      ctx.lineTo(xFor(wins[i].t_center_s), yFor(wins[i].point))
      ctx.stroke()
    }
    for (const x of wins) {
      ctx.fillStyle = x.provisional ? '#9a9a9a' : (QCOLOR[x.quality] ?? '#0a9a4a')
      ctx.beginPath(); ctx.arc(xFor(x.t_center_s), yFor(x.point), 2.5, 0, Math.PI * 2); ctx.fill()
    }
    ctx.fillStyle = '#777'
    ctx.font = '11px sans-serif'
    ctx.fillText(
      live ? `RMSSD (ms) - last ${RMSSD_VIEW}s, fixed 0-${HI} - gray edge still forming`
           : `RMSSD (ms) - point + uncertainty band, fixed 0-${HI}`,
      6, 14,
    )
  }

  function scrubFrom(ev: MouseEvent, canvas: HTMLCanvasElement) {
    if (mode === 'live') return
    const rect = canvas.getBoundingClientRect()
    const frac = (ev.clientX - rect.left) / rect.width
    const t = frac * tMax()
    stripStart = Math.max(0, Math.min(t - STRIP_WIN / 2, Math.max(0, tMax() - STRIP_WIN)))
  }

  function goLive() {
    if (!send || !selected) return
    error = ''
    resetLive()
    paused = false
    wantLive = true
    mode = 'live'
    send({ t: 'start', source: selected, mode: 'hrv', duration_s: 0 })
  }
  function stopLive() {
    if (send && selected) send({ t: 'stop', source: selected })
    wantLive = false
    paused = false
  }
  function togglePause() {
    if (paused) { scope.requestAnchor(); paused = false }
    else paused = true
  }
  function toLive() {
    mode = 'live'
  }
  function toSample() {
    if (running) stopLive()
    mode = 'sample'
    if (!bundle || bundle._axisRight !== undefined) loadSample()
  }

  onMount(() => {
    if (register) register(onFrame)
    loadSample()
    let raf = 0
    const loop = () => {
      const perf = performance.now()
      liveFresh = perf - lastHrMs < 2500 || perf - scope.lastEcgMs < 2500
      hrFresh = perf - lastHrMs < 3000
      if (mode === 'live') {
        scope.tick(perf, wantLive && !paused)
        if (perf - lastWinMs > 350) {
          liveWins = computeLiveWindows(rrTimes, rrVals, scope.ecgNewestT)
          lastWinMs = perf
        }
        bundle = buildLiveBundle()
      }
      drawStrip(); drawTacho(); drawRmssd()
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  })

  function fmtBand(s: any): string {
    if (!s || s.rmssd_point_ms == null) return '-'
    const [lo, hi] = s.rmssd_band_ms ?? [null, null]
    return `${s.rmssd_point_ms} ms  (band ${lo}-${hi})`
  }
</script>

<div class="hrv">
  <div class="topbar">
    <span class="tag">EXPLORATORY - NOT DIAGNOSTIC</span>

    <div class="modeswitch">
      <button class:active={mode === 'sample'} onclick={toSample}>SAMPLE</button>
      <button class:active={mode === 'live'} onclick={toLive}>LIVE</button>
    </div>

    {#if mode === 'live'}
      <select bind:value={selected} disabled={running}>
        {#each sourceIds as id}
          <option value={id}>{friendlyLabel(id)}</option>
        {/each}
      </select>
      {#if !running}
        <button class="go" onclick={goLive} disabled={!selected}>GO LIVE</button>
      {:else}
        <button class="stop" onclick={stopLive}>STOP</button>
        <button class="pause" onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
        <span class="speeds">
          {#each SPEEDS as s}
            <button class:active={speed === s} onclick={() => (speed = s)}>{s}</button>
          {/each}
          <span class="unit">mm/s</span>
        </span>
      {/if}
      <span class="metric beat">
        <span class="heart" class:on={hrFresh}>&hearts;</span>
        <b>{instHr ?? '--'}</b> bpm &middot; RR <b>{lastRr ?? '--'}</b> ms
      </span>
      <span class="metric live-state">
        {#if running && liveFresh && !hrFresh}waiting for native RR (HR){:else}{liveFresh ? 'streaming' : (liveStatus?.state ?? 'idle')}{/if}
      </span>
    {/if}

    {#if bundle?.session}
      <span class="metric">RMSSD{mode === 'live' ? ' (30s)' : ''}: <b>{fmtBand(bundle.session.native)}</b></span>
      <span class="metric">SDNN: <b>{bundle.session.native?.sdnn_ms ?? '-'}</b> ms</span>
      <span class="metric">corrected: <b>{bundle.session.native?.pct_corrected ?? '-'}%</b></span>
      {#if bundle.session.ecg_derived}
        <span class="metric">ECG-derived RMSSD: <b>{fmtBand(bundle.session.ecg_derived)}</b></span>
      {/if}
    {/if}
  </div>

  {#if error && mode === 'sample'}
    <div class="err">Could not load HRV bundle: {error}</div>
  {:else if mode === 'sample' && !bundle}
    <div class="err">Loading HRV bundle...</div>
  {:else}
    <div class="panel">
      <div class="label">
        RHYTHM STRIP - {mode === 'live' ? speed + ' mm/s' : '25 mm/s grid'}{#if paused} - PAUSED{/if}
        {#if mode === 'live'}- red R-peak, amber ectopic{:else}- R-peaks red, flagged beats amber{/if}
      </div>
      <canvas bind:this={stripCanvas} class="c strip"></canvas>
      {#if mode === 'sample'}
        <input
          type="range" min="0" max={Math.max(0, tMax() - STRIP_WIN)} step="0.2"
          bind:value={stripStart} class="scrub" />
        <div class="hint">t = {stripStart.toFixed(1)}-{(stripStart + STRIP_WIN).toFixed(1)} s of {tMax().toFixed(0)} s</div>
      {/if}
    </div>
    <div class="panel">
      <div class="label">TACHOGRAM - RR intervals{#if mode === 'live'} (native green vs ECG-derived red, scrolls with the strip){:else} (click to scrub the strip){/if}</div>
      <canvas bind:this={tachoCanvas} class="c tacho"
        onclick={(e) => scrubFrom(e, tachoCanvas)}></canvas>
    </div>
    <div class="panel">
      <div class="label">ROLLING RMSSD - band widens where beats are uncertain; dashed line = SDNN{#if mode === 'sample'} (click to scrub){/if}</div>
      <canvas bind:this={rmssdCanvas} class="c rmssd"
        onclick={(e) => scrubFrom(e, rmssdCanvas)}></canvas>
    </div>
  {/if}
</div>

<style>
  .hrv {
    height: calc(100vh - 58px);
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 14px 14px;
    box-sizing: border-box;
    overflow: auto;
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 13px;
    flex-wrap: wrap;
  }
  .tag {
    font-family: var(--font-display, sans-serif);
    letter-spacing: 1.5px;
    font-size: 12px;
    color: #fff;
    background: var(--color-heart, #cb333b);
    padding: 3px 10px;
    border-radius: 6px;
  }
  .modeswitch { display: flex; border: 1px solid var(--color-line, #ccc); border-radius: 6px; overflow: hidden; }
  .modeswitch button {
    border: 0; background: #fff; padding: 4px 12px; cursor: pointer;
    font-family: var(--font-display, sans-serif); font-size: 12px; letter-spacing: 1px;
    color: var(--color-slate, #555);
  }
  .modeswitch button.active { background: var(--color-ink, #111); color: #fff; }
  select { padding: 4px 6px; border: 1px solid var(--color-line, #ccc); border-radius: 6px; }
  .go, .stop {
    border: 0; border-radius: 6px; padding: 5px 14px; cursor: pointer; color: #fff;
    font-family: var(--font-display, sans-serif); letter-spacing: 1px; font-size: 12px;
  }
  .go { background: var(--color-good, #0a9a4a); }
  .stop { background: var(--color-heart, #cb333b); }
  .go:disabled { opacity: 0.5; cursor: default; }
  .live-state { text-transform: uppercase; font-size: 11px; letter-spacing: 0.5px; }
  .pause { background: var(--color-slate, #555); }
  .speeds { display: inline-flex; align-items: center; gap: 3px; }
  .speeds button {
    border: 1px solid var(--color-line, #ccc); background: #fff; border-radius: 5px;
    padding: 3px 8px; cursor: pointer; font-size: 12px; color: var(--color-slate, #555);
  }
  .speeds button.active { background: var(--color-ink, #111); color: #fff; border-color: var(--color-ink, #111); }
  .speeds .unit { font-size: 11px; color: var(--color-slate, #888); }
  .beat .heart { color: #d0d0d0; transition: color 0.1s; }
  .beat .heart.on { color: var(--color-heart, #cb333b); }
  .metric { color: var(--color-slate, #555); }
  .metric b { color: var(--color-ink, #111); }
  .panel { display: flex; flex-direction: column; gap: 4px; }
  .label {
    font-family: var(--font-display, sans-serif);
    font-size: 12px;
    letter-spacing: 1px;
    color: var(--color-slate, #777);
  }
  .c {
    width: 100%;
    background: #fff;
    border: 1px solid var(--color-line, #e3e0d8);
    border-radius: 8px;
  }
  .strip { height: 156px; }
  .tacho { height: 150px; cursor: crosshair; }
  .rmssd { height: 140px; cursor: crosshair; }
  .scrub { width: 100%; }
  .hint { font-size: 11px; color: var(--color-slate, #999); }
  .err { padding: 20px; color: var(--color-slate, #777); }
</style>

