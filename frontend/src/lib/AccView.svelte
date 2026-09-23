<script lang="ts">
  import { onMount } from 'svelte'
  import type { EcgStreamMsg } from './types'
  import { EcgScope } from './ecgScope'

  // RAW ACC (finpuss 21.09.2026): som RAW ECG - ingen kildevelger, viser
  // automatisk strømmen(e) som kjører, stablet ved dual-H10. De tre aksene
  // ligger oppå hverandre i ett komprimert panel per belte (~samme høyde som
  // EKG-strimmelen). Hver kilde har sin egen EcgScope-instans som inntar
  // EKG-rammer KUN for klokkeanker (host<->elapsed) og motor.

  interface Props {
    sources: Record<string, string>
    belts?: Record<string, string>
    register: (fn: (m: EcgStreamMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, belts = {}, register, onstatus }: Props = $props()

  // Enhetsnavn per kilde: status (benkeagent) OG telemetri (ESP32).
  let deviceSeen: Record<string, string> = $state({})
  function beltOf(src: string): string {
    const dev = deviceSeen[src] || onstatus(src)?.device || ''
    for (const [id, letter] of Object.entries(belts)) {
      if (dev.includes(id)) return letter
    }
    return ''
  }
  const PLACEMENT: Record<string, string> = {
    A: 'øvre belte - senter ~5 cm under høyre brystvorte',
    B: 'nedre belte, rotert - senter ~8 cm under venstre brystvorte',
  }

  const ACC_FS = 200
  const BUF_S = 60
  const SPEEDS = [25, 50]
  const MAX_PANELS = 2
  const accCap = ACC_FS * BUF_S

  type LaneBuf = {
    scope: EcgScope
    x: Float32Array
    y: Float32Array
    z: Float32Array
    t: Float64Array
    head: number
    filled: number
    lastT: number
    total: number
    seenResetSeq: number
  }
  const lanes: Record<string, LaneBuf> = {}
  function laneFor(src: string): LaneBuf {
    return (lanes[src] ??= {
      scope: new EcgScope(),
      x: new Float32Array(accCap),
      y: new Float32Array(accCap),
      z: new Float32Array(accCap),
      t: new Float64Array(accCap),
      head: 0,
      filled: 0,
      lastT: -Infinity,
      total: 0,
      seenResetSeq: 0,
    })
  }
  function resetLaneBuffers(l: LaneBuf) {
    l.x = new Float32Array(accCap)
    l.y = new Float32Array(accCap)
    l.z = new Float32Array(accCap)
    l.t = new Float64Array(accCap)
    l.head = 0
    l.filled = 0
    l.lastT = -Infinity
    l.total = 0
  }

  let paused = $state(false)
  let speed = $state(25)
  let active: string[] = $state([])
  let canvases: (HTMLCanvasElement | undefined)[] = $state([undefined, undefined])
  let mirror: { total: number; fresh: boolean }[] = $state([])

  const lastSeen: Record<string, number> = {}
  const streamingSince: Record<string, number> = {}

  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    if (id.startsWith('esp32-')) return 'ESP32 - Polar H10'
    return `native agent (${id.split(':')[0]})`
  }
  function orderKey(id: string): string {
    const b = beltOf(id)
    if (b) return '0' + b
    if (id.startsWith('esp32-')) return '1' + id
    if (id === 'raven:hci1') return '2' + id
    return '3' + id
  }
  function computeActive(perf: number): string[] {
    const ids = Object.keys(sources).filter((id) => id !== 'synth')
    const act = ids.filter((id) => {
      const st = onstatus(id)
      const fresh = perf - (lastSeen[id] ?? 0) < 3000
      return fresh || st?.state === 'streaming'
    })
    act.sort((a, b) => orderKey(a).localeCompare(orderKey(b)))
    return act.slice(0, MAX_PANELS)
  }

  onMount(() => {
    register((m: EcgStreamMsg) => {
      const src = (m as any).source as string
      if (src === 'synth') return
      if ((m as any).t === 'telemetry') {
        const dev = (m as any).device
        if (dev && deviceSeen[src] !== dev) deviceSeen[src] = dev
        return
      }
      const l = laneFor(src)
      if (m.t === 'ecg') {
        // Kun klokkeanker + motor; EKG-kurven vises i RAW ECG-fanen.
        l.scope.ingestEcg(m as any)
        lastSeen[src] = performance.now()
        if (l.scope.resetSeq !== l.seenResetSeq) {
          l.seenResetSeq = l.scope.resetSeq
          resetLaneBuffers(l)
        }
      } else if (m.t === 'acc') {
        lastSeen[src] = performance.now()
        const E = l.scope.elapsedOf(m as any)
        if (Number.isNaN(l.scope.hostElapsedOffset)) return
        const s = (m as any).samples as number[][]
        if (!s.length) return
        const base = E - (s.length - 1) / ACC_FS
        let shift = 0
        if (l.lastT > -Infinity) {
          const overlap = l.lastT + 1 / ACC_FS - base
          if (overlap > 0 && overlap < 0.5) shift = overlap
        }
        for (let i = 0; i < s.length; i++) {
          l.x[l.head] = s[i][0]
          l.y[l.head] = s[i][1]
          l.z[l.head] = s[i][2]
          l.t[l.head] = base + i / ACC_FS + shift
          l.head = (l.head + 1) % accCap
          if (l.filled < accCap) l.filled++
        }
        l.lastT = base + (s.length - 1) / ACC_FS + shift
        l.total = (m as any).total
      }
    })
    let raf = 0
    const loop = () => {
      const perf = performance.now()
      const act = computeActive(perf)
      if (act.join('|') !== active.join('|')) active = act
      const mir: typeof mirror = []
      act.forEach((src, i) => {
        const l = laneFor(src)
        const fresh = perf - l.scope.lastEcgMs < 1500
        const st = onstatus(src)
        const live = st?.state === 'streaming'
        if (live && !streamingSince[src]) streamingSince[src] = perf
        if (!live) streamingSince[src] = 0
        l.scope.tick(perf, fresh && !paused)
        mir.push({ total: l.total, fresh })
        const canvas = canvases[i]
        if (canvas) drawAcc(canvas, l, src, live, perf)
      })
      mirror = mir
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  })

  function togglePause() {
    if (paused) {
      for (const l of Object.values(lanes)) l.scope.requestAnchor()
      paused = false
    } else paused = true
  }

  function drawAcc(canvas: HTMLCanvasElement, l: LaneBuf, src: string, live: boolean, perf: number) {
    const dpr = window.devicePixelRatio || 1
    const w = canvas.clientWidth
    const h = canvas.clientHeight
    if (!w || !h) return
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
    }
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)

    const mmPx = l.scope.PX_PER_MM
    const winS = w / mmPx / speed
    const t1 = l.scope.nowInit ? l.scope.nowT : winS
    const t0 = t1 - winS
    const xFor = (t: number) => (t - t0) * speed * mmPx
    const collect = (v: Float32Array) => {
      const vs: number[] = [], ts: number[] = []
      for (let i = 0; i < l.filled; i++) {
        const idx = (l.head - l.filled + i + accCap * 2) % accCap
        const tt = l.t[idx]
        if (tt < t0 - 0.2 || tt > t1) continue
        ts.push(tt); vs.push(v[idx])
      }
      return { vs, ts }
    }
    const gx = collect(l.x), gy = collect(l.y), gz = collect(l.z)
    if (gx.vs.length < 2) {
      const cx = w / 2, cy = h / 2
      const tsec = live && streamingSince[src] ? (perf - streamingSince[src]) / 1000 : 0
      const pulse = 0.5 + 0.5 * Math.sin(perf / 350)
      ctx.save()
      ctx.textAlign = 'center'
      if (live) {
        ctx.fillStyle = `rgba(203,51,59,${0.2 + 0.55 * pulse})`
        ctx.beginPath(); ctx.arc(cx, cy - 16, 8 + 4 * pulse, 0, Math.PI * 2); ctx.fill()
      }
      ctx.fillStyle = '#444'
      ctx.font = 'bold 15px sans-serif'
      ctx.fillText(live ? 'Polar H10 preparing signal...' : 'venter på strøm', cx, cy + 14)
      if (live) {
        ctx.fillStyle = '#888'
        ctx.font = '12px sans-serif'
        ctx.fillText(`waiting for first ACC frame - ${tsec.toFixed(0)} s`, cx, cy + 34)
      }
      ctx.textAlign = 'left'
      ctx.restore()
      return
    }
    let min = Infinity, max = -Infinity
    for (const arr of [gx.vs, gy.vs, gz.vs]) for (const v of arr) { if (v < min) min = v; if (v > max) max = v }
    const pad = Math.max(100, (max - min) * 0.1)
    min -= pad; max += pad
    const yFor = (v: number) => h - ((v - min) / (max - min)) * h
    const line = (g: { vs: number[]; ts: number[] }, color: string) => {
      ctx.strokeStyle = color; ctx.lineWidth = 1.2; ctx.beginPath()
      let started = false, prevT = -Infinity
      for (let i = 0; i < g.vs.length; i++) {
        const x = xFor(g.ts[i]), y = yFor(g.vs[i])
        if (!started || g.ts[i] - prevT > 1.8 / ACC_FS) { ctx.moveTo(x, y); started = true }
        else ctx.lineTo(x, y)
        prevT = g.ts[i]
      }
      ctx.stroke()
    }
    line(gx, '#CB333B'); line(gy, '#40A15D'); line(gz, '#778395')
  }
</script>

<div class="acc">
  <div class="bar">
    <button class="pause" disabled={!active.length && !paused} onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
    <div class="speed">
      {#each SPEEDS as s (s)}
        <button class="sp" class:on={speed === s} onclick={() => (speed = s)}>{s}</button>
      {/each}
      <span class="unit">mm/s</span>
    </div>
    {#if !active.length}
      <span class="state">ingen aktiv strøm - start økten fra TILKOBLING-fanen</span>
    {/if}
  </div>

  {#if active.length}
    {#each active as src, i (src)}
      {@const st = onstatus(src)}
      {@const m = mirror[i]}
      <div class="panelwrap">
        <div class="striphead">
          {#if beltOf(src)}
            <span class="belt" title={PLACEMENT[beltOf(src)] ?? ''}>BELTE {beltOf(src)}</span>
          {/if}
          <b>{friendlyLabel(src)}</b>
          {#if deviceSeen[src] || st?.device}<span class="dev">{deviceSeen[src] || st?.device}</span>{/if}
          <span class="stats">
            <span class="state" class:err={st?.state === 'error'}>{st ? st.state + (st.detail ? ' - ' + st.detail : '') : ''}</span>
            <span>{(m?.total ?? 0).toLocaleString()} samples</span>
            <span>{((m?.total ?? 0) / ACC_FS).toFixed(1)} s</span>
          </span>
        </div>
        <div class="scope accscope">
          <canvas bind:this={canvases[i]}></canvas>
          <div class="scale">
            ACC - 200 Hz - milli-g -
            <span style="color:#CB333B">X</span>
            <span style="color:#40A15D">Y</span>
            <span style="color:#778395">Z</span>
          </div>
        </div>
      </div>
    {/each}
  {:else}
    <div class="scope accscope idle">
      <div class="idlemsg">
        Ingen aktiv strøm. Start økten fra <b>TILKOBLING</b>-fanen - panelene
        dukker opp her av seg selv (ett per belte ved dual-H10).
      </div>
    </div>
  {/if}
</div>

<style>
  .acc {
    height: calc(100vh - 58px);
    display: flex;
    flex-direction: column;
    padding: 14px;
    box-sizing: border-box;
    gap: 12px;
    overflow-y: auto;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  button {
    font-family: var(--font-display);
    font-size: 17px;
    letter-spacing: 2px;
    padding: 7px 26px;
    border: none;
    border-radius: 10px;
    cursor: pointer;
    color: #fff;
  }
  button.pause {
    background: var(--color-slate);
  }
  button.pause:disabled {
    background: var(--color-disabled);
    cursor: not-allowed;
  }
  .speed {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .speed .sp {
    font-family: var(--font-body);
    font-size: 12px;
    letter-spacing: 0;
    padding: 4px 10px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-card);
    color: var(--color-slate);
  }
  .speed .sp.on {
    background: var(--color-slate);
    color: #fff;
    border-color: var(--color-slate);
  }
  .speed .unit {
    font-size: 12px;
    color: var(--color-slate);
  }
  .panelwrap { display: flex; flex-direction: column; gap: 6px; }
  .striphead {
    display: flex;
    align-items: baseline;
    gap: 14px;
    font-size: 13.5px;
    color: var(--color-slate);
    flex-wrap: wrap;
  }
  .striphead > b {
    font-family: var(--font-display);
    letter-spacing: 1px;
    color: var(--color-ink, #222);
    font-size: 13px;
  }
  .belt {
    font-family: var(--font-display);
    font-size: 11px;
    letter-spacing: 1.5px;
    color: #fff;
    background: var(--color-slate, #555);
    padding: 2px 8px;
    border-radius: 5px;
    cursor: help;
  }
  .striphead .dev { font-size: 12px; }
  .stats { display: flex; gap: 14px; flex-wrap: wrap; }
  .state { text-transform: lowercase; }
  .state.err { color: var(--color-error); }
  .scope {
    position: relative;
    background: var(--color-card);
    border: 1px solid var(--color-line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .accscope {
    /* Komprimert: ~samme høyde som EKG-strimmelen (finpuss 21.09), stables. */
    flex: 0 0 220px;
  }
  .accscope.idle { display: flex; align-items: center; justify-content: center; }
  .idlemsg { color: var(--color-slate); font-size: 14px; max-width: 480px; text-align: center; }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .scale {
    position: absolute;
    right: 12px;
    bottom: 10px;
    font-size: 13px;
    color: var(--color-slate);
    background: rgba(243, 241, 236, 0.7);
    padding: 2px 8px;
    border-radius: 6px;
  }
</style>
