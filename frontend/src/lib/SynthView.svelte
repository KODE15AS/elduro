<script lang="ts">
  import { onMount } from 'svelte'
  import { EcgScope } from './ecgScope'

  // SYNTETISK EKG (21.09.2026, erstatter RHYTHM/HRV-fanen etter beslutning):
  // viser det backend-fusjonerte «syntetisk estimert EKG» fra dual-H10
  // (kilde "synth"), alltid live, uten kildevelger. Grunnlaget (A+B eller én
  // avledning solo) og konfidens vises åpent; strimmelen tegnes «washed out»
  // mot høyre kant der estimatet ennå ikke er modent (avtalt med Jørn).
  // Tachogram (nativ RR grønn vs EKG-derivert rød) og rullende RMSSD består
  // som før - nativ RR hentes fra det ferskeste beltet (A foretrukket).

  // Kun rammefeeden brukes; øktstyring og kildevalg finnes ikke her lenger.
  interface Props {
    register?: (fn: (m: any) => void) => void
  }
  let { register }: Props = $props()

  const TACHO_WIN = 20
  const RMSSD_VIEW = 120
  const RMSSD_HALF = 15
  const RMSSD_STEP = 2
  const RR_LO = 400, RR_HI = 1200
  const RMSSD_HI = 150
  const PX_PER_MM = 96 / 25.4
  const SPEEDS = [25, 50]

  // Samme kliniske motor som RAW ECG - matet med den syntetiske strømmen.
  const scope = new EcgScope()

  let lastRr = $state<number | null>(null)
  let instHr = $state<number | null>(null)
  let liveFresh = $state(false)
  let hrFresh = $state(false)
  let basis = $state('')
  let conf = $state(0)
  let residualMs = $state<number | null>(null)
  let driftPpm = $state<number | null>(null)
  let hrSource = $state('')

  let stripCanvas: HTMLCanvasElement
  let tachoCanvas: HTMLCanvasElement
  let rmssdCanvas: HTMLCanvasElement

  // Nativ-RR-akkumulatorer (den syntetiske EKG-en bor i `scope`).
  let rrVals: number[] = []
  let rrTimes: number[] = []
  let lastHrMs = 0
  const hrSeen: Record<string, number> = {} // src -> perf ms for siste hr-ramme

  let speed = $state(25)
  let paused = $state(false)
  let liveWins: any[] = []
  let lastWinMs = 0
  let bundle: any = null
  let seenResetSeq = 0

  function friendlyBasis(b: string): string {
    if (b === 'A+B') return 'belte A + belte B (fusjon)'
    if (b.startsWith('esp32-')) return 'kun belte A / ESP32 (ingen fusjon)'
    if (b.startsWith('raven:')) return 'kun belte B / BT-600 (ingen fusjon)'
    return b ? `kun ${b} (ingen fusjon)` : '-'
  }
  // Nativ RR: foretrekk ESP32 (belte A) når fersk, ellers ferskeste kilde.
  function pickHrSource(perf: number): string {
    const fresh = Object.entries(hrSeen).filter(([, t]) => perf - t < 4000)
    if (!fresh.length) return ''
    const esp = fresh.find(([s]) => s.startsWith('esp32-'))
    if (esp) return esp[0]
    fresh.sort((a, b) => b[1] - a[1])
    return fresh[0][0]
  }

  function onFrame(m: any) {
    if (m.t === 'ecg' && m.source === 'synth') {
      scope.ingestEcg(m)
      if (m.synth) {
        basis = m.synth.basis ?? ''
        conf = m.synth.conf ?? 0
        residualMs = m.synth.residual_ms ?? null
        driftPpm = m.synth.drift_ppm ?? null
      }
      if (scope.resetSeq !== seenResetSeq) {
        seenResetSeq = scope.resetSeq
        rrVals = []; rrTimes = []; liveWins = []; lastRr = null; instHr = null
      }
    } else if (m.t === 'hr' && m.source !== 'synth') {
      const perf = performance.now()
      hrSeen[m.source] = perf
      if (m.source !== hrSource) return // kun valgt kilde mater tachogrammet
      lastHrMs = perf
      const rr = (m.rr as number[]) ?? []
      if (!rr.length) return
      // Anker slagene ved den syntetiske strimmelens ferskeste tid (ulike
      // kilder har ulike elapsed-klokker; synth-tidslinjen er fellesnevneren).
      const tEnd = scope.ecgNewestT
      if (tEnd <= 0) return
      let acc = 0
      const tail = rr.map((v) => (acc += v))
      const total = acc
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

  // ---------- HRV-matematikk (speiler analysis/hrv.py) ----------
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

  function buildBundle() {
    const rr = rrVals.slice()
    const times = rrTimes.slice()
    const trailIdx: number[] = []
    for (let i = 0; i < times.length; i++) if (times[i] >= scope.ecgNewestT - 30) trailIdx.push(i)
    const sess = bandOf(trailIdx.map((i) => rr[i]))
    const pk = scope.peaks
    const ecgRrT: number[] = []
    const ecgRrV: number[] = []
    for (let i = 1; i < pk.length; i++) {
      if (pk[i - 1].t < scope.ecgNewestT - (TACHO_WIN + 6)) continue
      ecgRrT.push(pk[i].t)
      ecgRrV.push((pk[i].t - pk[i - 1].t) * 1000)
    }
    return {
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
      _axisRight: scope.nowT,
    }
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
    const f = fit(stripCanvas)
    if (!f) return
    const [ctx, w, h] = f
    const drew = scope.drawScope(ctx, w, h, speed)

    if (drew < 2) {
      const cx = w / 2, cy = h / 2
      ctx.save()
      ctx.textAlign = 'center'
      ctx.fillStyle = '#444'
      ctx.font = 'bold 16px sans-serif'
      ctx.fillText('venter på syntetisk strøm', cx, cy + 4)
      ctx.fillStyle = '#888'
      ctx.font = '13px sans-serif'
      ctx.fillText('krever minst én aktiv EKG-strøm - start økten fra TILKOBLING-fanen', cx, cy + 26)
      ctx.textAlign = 'left'
      ctx.restore()
      return
    }

    // «Washed out» mot høyre kant: estimatet er ferskest (og minst modent)
    // lengst til høyre. Bredden styres av konfidensen - lav konfidens = bredere
    // vask. Avtalt visualisering (Jørn 21.09).
    const washFrac = Math.min(0.45, 0.08 + (1 - conf) * 0.3)
    const x0 = w * (1 - washFrac)
    const grad = ctx.createLinearGradient(x0, 0, w, 0)
    grad.addColorStop(0, 'rgba(243,241,236,0)')
    grad.addColorStop(1, `rgba(243,241,236,${0.55 + 0.35 * (1 - conf)})`)
    ctx.fillStyle = grad
    ctx.fillRect(x0, 0, w - x0, h)

    if (scope.poorState || scope.nowT - scope.lastPeakT > 2) {
      ctx.fillStyle = '#b06a00'
      ctx.font = 'bold 13px sans-serif'
      ctx.fillText('NO SIGNAL', w - 96, 22)
    }
  }

  function drawTacho() {
    if (!tachoCanvas || !bundle) return
    const f = fit(tachoCanvas)
    if (!f) return
    const [ctx, w, h] = f

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
    ctx.fillText(`RR (ms) - nativ H10 (grønn), EKG-derivert fra syntetisk (rød stiplet) - ${winS.toFixed(0)}s @ ${speed} mm/s`, 30, 14)
  }

  function drawRmssd() {
    if (!rmssdCanvas || !bundle?.windows) return
    const f = fit(rmssdCanvas)
    if (!f) return
    const [ctx, w, h] = f
    const t1 = bundle._axisRight
    const t0 = t1 - RMSSD_VIEW
    const wins = bundle.windows.filter(
      (x: any) => x.quality !== 'no_signal' && !isNaN(x.point) && x.t_center_s >= t0 && x.t_center_s <= t1,
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
    ctx.fillText(`RMSSD (ms) - siste ${RMSSD_VIEW}s, fast 0-${HI} - grå kant = under dannelse`, 6, 14)
  }

  function togglePause() {
    if (paused) { scope.requestAnchor(); paused = false }
    else paused = true
  }

  onMount(() => {
    if (register) register(onFrame)
    let raf = 0
    const loop = () => {
      const perf = performance.now()
      hrSource = pickHrSource(perf)
      liveFresh = perf - scope.lastEcgMs < 2500
      hrFresh = perf - lastHrMs < 3000
      scope.tick(perf, liveFresh && !paused)
      if (perf - lastWinMs > 350) {
        liveWins = computeLiveWindows(rrTimes, rrVals, scope.ecgNewestT)
        lastWinMs = perf
      }
      bundle = buildBundle()
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
  let sessNative: any = $state(null)
  $effect(() => {
    const iv = setInterval(() => {
      sessNative = bundle?.session?.native ?? null
    }, 500)
    return () => clearInterval(iv)
  })
</script>

<div class="hrv">
  <div class="topbar">
    <span class="tag">SYNTETISK ESTIMERT EKG - IKKE DIAGNOSTISK</span>
    <span class="metric basis">
      grunnlag: <b>{friendlyBasis(basis)}</b>
      {#if liveFresh && basis === 'A+B'}
        &middot; konfidens <b>{(conf * 100).toFixed(0)} %</b>
        {#if residualMs != null && !isNaN(residualMs)}&middot; synk-residual <b>{residualMs.toFixed(1)} ms</b>{/if}
        {#if driftPpm != null && !isNaN(driftPpm)}&middot; drift <b>{driftPpm.toFixed(0)} ppm</b>{/if}
      {/if}
    </span>
    <button class="pause" disabled={!liveFresh && !paused} onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
    <span class="speeds">
      {#each SPEEDS as s (s)}
        <button class:active={speed === s} onclick={() => (speed = s)}>{s}</button>
      {/each}
      <span class="unit">mm/s</span>
    </span>
    <span class="metric beat">
      <span class="heart" class:on={hrFresh}>&hearts;</span>
      <b>{instHr ?? '--'}</b> bpm &middot; RR <b>{lastRr ?? '--'}</b> ms
      {#if hrSource}<span class="dim">(nativ RR: {hrSource.startsWith('esp32-') ? 'belte A' : 'belte B'})</span>{/if}
    </span>
    {#if sessNative}
      <span class="metric">RMSSD (30s): <b>{fmtBand(sessNative)}</b></span>
      <span class="metric">SDNN: <b>{sessNative?.sdnn_ms ?? '-'}</b> ms</span>
      <span class="metric">korrigert: <b>{sessNative?.pct_corrected ?? '-'}%</b></span>
    {/if}
    <span class="metric live-state">
      {liveFresh ? 'live' : 'venter - start økt fra TILKOBLING-fanen'}
    </span>
  </div>

  <div class="panel">
    <div class="label">
      SYNTETISK STRIMMEL - {speed} mm/s - 10 mm/mV{#if paused} - PAUSED{/if}
      - rød R-topp, oransje ektopisk - vasket høyrekant = estimat under dannelse
    </div>
    <canvas bind:this={stripCanvas} class="c strip"></canvas>
  </div>
  <div class="panel">
    <div class="label">TACHOGRAM - RR-intervaller (nativ grønn vs EKG-derivert rød, ruller med strimmelen)</div>
    <canvas bind:this={tachoCanvas} class="c tacho"></canvas>
  </div>
  <div class="panel">
    <div class="label">RULLENDE RMSSD - båndet vider seg der slagene er usikre; stiplet linje = SDNN</div>
    <canvas bind:this={rmssdCanvas} class="c rmssd"></canvas>
  </div>
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
  .basis { font-size: 13px; }
  .live-state { text-transform: uppercase; font-size: 11px; letter-spacing: 0.5px; }
  .pause {
    border: 0; border-radius: 6px; padding: 5px 14px; cursor: pointer; color: #fff;
    font-family: var(--font-display, sans-serif); letter-spacing: 1px; font-size: 12px;
    background: var(--color-slate, #555);
  }
  .pause:disabled { opacity: 0.5; cursor: default; }
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
  .metric .dim { color: var(--color-slate, #999); font-size: 11px; }
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
  .tacho { height: 150px; }
  .rmssd { height: 140px; }
</style>
