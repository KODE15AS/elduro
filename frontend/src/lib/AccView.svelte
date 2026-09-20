<script lang="ts">
  import { onMount } from 'svelte'
  import type { EcgStreamMsg } from './types'
  import { EcgScope } from './ecgScope'

  // RAW ACC (20.09.2026): ACC-skopet flyttet ut av RAW ECG for å gjøre plass
  // til to EKG-strimler (dual-H10). Bruker sin egen EcgScope-instans som
  // inntar EKG-rammer KUN for klokkeanker (host<->elapsed-offset) og motor;
  // selve EKG-kurven tegnes ikke her.
  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: EcgStreamMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, send, register, onstatus }: Props = $props()

  const ACC_FS = 200
  const BUF_S = 60
  const SPEEDS = [25, 50]

  const scope = new EcgScope()

  let selected = $state('')
  let streamingSince = 0
  let paused = $state(false)
  let speed = $state(25)
  let accTotal = $state(0)
  let ecgFresh = $state(false)
  let accCanvas: HTMLCanvasElement | undefined = $state()

  const accCap = ACC_FS * BUF_S
  let accX = new Float32Array(accCap)
  let accY = new Float32Array(accCap)
  let accZ = new Float32Array(accCap)
  let accT = new Float64Array(accCap)
  let accHead = 0
  let accFilled = 0
  let accLastT = -Infinity

  const sourceIds = $derived(Object.keys(sources))
  const status = $derived(selected ? onstatus(selected) : null)
  const recording = $derived(ecgFresh)
  const live = $derived(!!status && status.state === 'streaming')

  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    if (id.startsWith('esp32-')) return 'ESP32 - Polar H10'
    return `native agent (${id.split(':')[0]})`
  }
  function preferredSource(ids: string[]): string {
    return ids.find((id) => !id.startsWith('raven:hci0')) ?? ids[0] ?? ''
  }

  $effect(() => {
    if (!selected && sourceIds.length) selected = preferredSource(sourceIds)
  })

  onMount(() => {
    register((m: EcgStreamMsg) => {
      if (m.source !== selected) return
      if (m.t === 'ecg') {
        // Kun klokkeanker + motor; kurven vises i RAW ECG-fanen.
        scope.ingestEcg(m)
      } else if (m.t === 'acc') {
        const E = scope.elapsedOf(m)
        if (Number.isNaN(scope.hostElapsedOffset)) return
        const s = m.samples as number[][]
        if (!s.length) return
        const base = E - (s.length - 1) / ACC_FS
        let shift = 0
        if (accLastT > -Infinity) {
          const overlap = accLastT + 1 / ACC_FS - base
          if (overlap > 0 && overlap < 0.5) shift = overlap
        }
        for (let i = 0; i < s.length; i++) {
          accX[accHead] = s[i][0]
          accY[accHead] = s[i][1]
          accZ[accHead] = s[i][2]
          accT[accHead] = base + i / ACC_FS + shift
          accHead = (accHead + 1) % accCap
          if (accFilled < accCap) accFilled++
        }
        accLastT = base + (s.length - 1) / ACC_FS + shift
        accTotal = m.total
      }
    })
    let raf = 0
    const loop = () => {
      const perf = performance.now()
      ecgFresh = perf - scope.lastEcgMs < 1500
      if (live && !streamingSince) streamingSince = perf
      if (!live) streamingSince = 0
      scope.tick(perf, ecgFresh && !paused)
      drawAcc()
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  })

  function resetBuffers() {
    scope.reset()
    accX = new Float32Array(accCap)
    accY = new Float32Array(accCap)
    accZ = new Float32Array(accCap)
    accT = new Float64Array(accCap)
    accHead = 0
    accFilled = 0
    accLastT = -Infinity
    accTotal = 0
  }

  // Bytte av kilde (dual-H10) gir et rent skop.
  $effect(() => {
    void selected
    resetBuffers()
  })

  function togglePause() {
    if (paused) { scope.requestAnchor(); paused = false }
    else paused = true
  }

  function drawAcc() {
    if (!accCanvas) return
    const dpr = window.devicePixelRatio || 1
    const w = accCanvas.clientWidth
    const h = accCanvas.clientHeight
    if (!w || !h) return
    if (accCanvas.width !== Math.round(w * dpr) || accCanvas.height !== Math.round(h * dpr)) {
      accCanvas.width = Math.round(w * dpr)
      accCanvas.height = Math.round(h * dpr)
    }
    const ctx = accCanvas.getContext('2d')
    if (!ctx) return
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, w, h)

    const mmPx = scope.PX_PER_MM
    const winS = w / mmPx / speed
    const t1 = scope.nowInit ? scope.nowT : winS
    const t0 = t1 - winS
    const xFor = (t: number) => (t - t0) * speed * mmPx
    const collect = (v: Float32Array) => {
      const vs: number[] = [], ts: number[] = []
      for (let i = 0; i < accFilled; i++) {
        const idx = (accHead - accFilled + i + accCap * 2) % accCap
        const tt = accT[idx]
        if (tt < t0 - 0.2 || tt > t1) continue
        ts.push(tt); vs.push(v[idx])
      }
      return { vs, ts }
    }
    const gx = collect(accX), gy = collect(accY), gz = collect(accZ)
    if (gx.vs.length < 2) {
      // Oppvarmings-/idle-overlay (H10 holder hele PMD-strømmen ~5-35 s).
      const cx = w / 2, cy = h / 2
      const tsec = live && streamingSince ? (performance.now() - streamingSince) / 1000 : 0
      const pulse = 0.5 + 0.5 * Math.sin(performance.now() / 350)
      ctx.save()
      ctx.textAlign = 'center'
      if (live) {
        ctx.fillStyle = `rgba(203,51,59,${0.2 + 0.55 * pulse})`
        ctx.beginPath(); ctx.arc(cx, cy - 16, 8 + 4 * pulse, 0, Math.PI * 2); ctx.fill()
      }
      ctx.fillStyle = '#444'
      ctx.font = 'bold 17px sans-serif'
      ctx.fillText(live ? 'Polar H10 preparing signal...' : 'start the session from the CONNECTION tab', cx, cy + 16)
      if (live) {
        ctx.fillStyle = '#888'
        ctx.font = '13px sans-serif'
        ctx.fillText(`waiting for first ACC frame - ${tsec.toFixed(0)} s`, cx, cy + 38)
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
    <label>
      Source
      <select bind:value={selected}>
        {#if !sourceIds.length}
          <option value="">no agent connected</option>
        {/if}
        {#each sourceIds as id (id)}
          <option value={id}>{friendlyLabel(id)}</option>
        {/each}
      </select>
    </label>
    <button class="pause" disabled={!recording && !paused} onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
    <div class="speed">
      {#each SPEEDS as s (s)}
        <button class="sp" class:on={speed === s} onclick={() => (speed = s)}>{s}</button>
      {/each}
      <span class="unit">mm/s</span>
    </div>
    <div class="stats">
      {#if status}
        <span class="state" class:err={status.state === 'error'}>
          {status.state}{status.detail ? ' - ' + status.detail : ''}
        </span>
      {:else}
        <span class="state">idle - start from the CONNECTION tab</span>
      {/if}
      <span title="Total ACC samples received this session">{accTotal.toLocaleString()} ACC samples</span>
      <span title="Seconds of ACC recorded">{(accTotal / ACC_FS).toFixed(1)} s rec</span>
    </div>
  </div>
  <div class="scope accscope">
    <canvas bind:this={accCanvas}></canvas>
    <div class="scale">
      ACC - 200 Hz - milli-g -
      <span style="color:#CB333B">X</span>
      <span style="color:#40A15D">Y</span>
      <span style="color:#778395">Z</span>
    </div>
  </div>
</div>

<style>
  .acc {
    height: calc(100vh - 58px);
    display: flex;
    flex-direction: column;
    padding: 14px;
    box-sizing: border-box;
    gap: 12px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }
  .bar select {
    margin-left: 6px;
    font-family: var(--font-body);
    padding: 4px 8px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-card);
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
  .stats {
    display: flex;
    gap: 16px;
    font-size: 13.5px;
    color: var(--color-slate);
    flex-wrap: wrap;
  }
  .stats .state {
    text-transform: lowercase;
  }
  .stats .state.err {
    color: var(--color-error);
  }
  .scope {
    position: relative;
    background: var(--color-card);
    border: 1px solid var(--color-line);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .accscope {
    flex: 1;
  }
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
