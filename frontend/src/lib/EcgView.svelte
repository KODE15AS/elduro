<script lang="ts">
  import { onMount } from 'svelte'
  import type { EcgStreamMsg } from './types'
  import { EcgScope } from './ecgScope'

  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: EcgStreamMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, send, register, onstatus }: Props = $props()

  const ECG_FS = 130
  const ACC_FS = 200
  const BUF_S = 60 // ACC ring length in seconds
  const SPEEDS = [25, 50] // mm/s selectable

  // Shared clinical-ECG engine: the RHYTHM/HRV tab runs this same code, so the
  // live ECG signal path (baseline, detection, motor, drawing) is identical.
  const scope = new EcgScope()

  let selected = $state('')
  let wantRec = $state(false)
  let recStartMs = 0 // when RECORD was pressed, for the warmup overlay timer
  let paused = $state(false)
  let speed = $state(25)
  let ecgTotal = $state(0)
  let accTotal = $state(0)
  let gaps = $state(0)
  let deviceTimeS = $state(0)
  let ecgFresh = $state(false)
  let hrBpm = $state<number | null>(null)
  let ecgCanvas: HTMLCanvasElement | undefined = $state()
  let accCanvas: HTMLCanvasElement | undefined = $state()

  // ACC ring buffers (host clock seconds), non-reactive. The ECG buffers live
  // inside `scope` and share its host<->elapsed clock offset.
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
  const recording = $derived(wantRec || ecgFresh)
  const busyOther = $derived(
    !!status && status.state === 'streaming' && !ecgFresh && !wantRec,
  )

  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    return `Lenovo - native agent (${id.split(':')[0]})`
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
        scope.ingestEcg(m)
        ecgTotal = scope.ecgTotal
        gaps = scope.gaps
        deviceTimeS = scope.deviceTimeS
      } else if (m.t === 'acc') {
        // ACC frames carry no elapsed_ms; wait until the ECG->host offset is known
        // so they land on the shared timeline (an early ACC frame would otherwise
        // get the Unix wall-clock and poison accLastT via the monotonic shift).
        const E = scope.elapsedOf(m)
        if (Number.isNaN(scope.hostElapsedOffset)) return
        const s = m.samples as number[][]
        if (!s.length) return
        const base = E - (s.length - 1) / ACC_FS
        let shift = 0
        if (accLastT > -Infinity) {
          const overlap = accLastT + 1 / ACC_FS - base
          if (overlap > 0 && overlap < 0.5) shift = overlap // only bridge small jitter
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
      // the motor runs only while recording and not paused; STOP/PAUSE freeze it
      scope.tick(perf, wantRec && !paused)
      hrBpm = scope.hrBpm
      drawEcg()
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
    ecgTotal = 0
    accTotal = 0
    gaps = 0
    hrBpm = null
  }

  function start() {
    if (!selected) return
    resetBuffers()
    paused = false
    wantRec = true
    recStartMs = performance.now()
    send({ t: 'start', source: selected, mode: 'ecg', duration_s: 0 })
  }
  function stop() {
    wantRec = false
    paused = false
    if (selected) send({ t: 'stop', source: selected })
  }
  function togglePause() {
    if (paused) { scope.requestAnchor(); paused = false }
    else paused = true
  }

  $effect(() => {
    if (status?.state === 'error') wantRec = false
  })

  function prepare(canvas: HTMLCanvasElement): [CanvasRenderingContext2D, number, number] | null {
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

  function drawEcg() {
    if (!ecgCanvas) return
    const p = prepare(ecgCanvas)
    if (!p) return
    const [ctx, w, h] = p
    const drew = scope.drawScope(ctx, w, h, speed)

    if (drew < 2) {
      // Warmup overlay: the H10 withholds its whole PMD stream (ECG + ACC)
      // until it enters "measuring" state, which can take ~30 s with dry
      // electrodes. Show a friendly status instead of a blank scrolling grid.
      const cx = w / 2, cy = h / 2
      const tsec = wantRec && recStartMs ? (performance.now() - recStartMs) / 1000 : 0
      const pulse = 0.5 + 0.5 * Math.sin(performance.now() / 350)
      ctx.save()
      ctx.textAlign = 'center'
      if (wantRec) {
        ctx.fillStyle = `rgba(203,51,59,${0.2 + 0.55 * pulse})`
        ctx.beginPath(); ctx.arc(cx, cy - 16, 8 + 4 * pulse, 0, Math.PI * 2); ctx.fill()
      }
      ctx.fillStyle = '#444'
      ctx.font = 'bold 17px sans-serif'
      ctx.fillText(wantRec ? 'Polar H10 preparing signal...' : 'press RECORD to start', cx, cy + 16)
      if (wantRec) {
        ctx.fillStyle = '#888'
        ctx.font = '13px sans-serif'
        ctx.fillText(
          `waiting for first ECG frame - ${tsec.toFixed(0)} s  (can take ~30 s with dry electrodes; moisten for a faster start)`,
          cx, cy + 38,
        )
      }
      ctx.textAlign = 'left'
      ctx.restore()
    }

    // NO SIGNAL when the live signal is too noisy or no beat has been seen lately
    if (drew >= 2 && (scope.poorState || scope.nowT - scope.lastPeakT > 2)) {
      ctx.fillStyle = '#b06a00'
      ctx.font = 'bold 13px sans-serif'
      ctx.fillText('NO SIGNAL', w - 96, 22)
    }
  }

  function drawAcc() {
    if (!accCanvas) return
    const p = prepare(accCanvas)
    if (!p) return
    const [ctx, w, h] = p
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
    if (gx.vs.length < 2) return
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

<div class="ecg">
  <div class="bar">
    <label>
      Source
      <select bind:value={selected} disabled={recording}>
        {#if !sourceIds.length}
          <option value="">no native agent connected</option>
        {/if}
        {#each sourceIds as id (id)}
          <option value={id}>{friendlyLabel(id)}</option>
        {/each}
      </select>
    </label>
    {#if recording}
      <button class="stop" onclick={stop}>STOP</button>
      <button class="pause" onclick={togglePause}>{paused ? 'RESUME' : 'PAUSE'}</button>
    {:else}
      <button class="rec" disabled={!selected} onclick={start}>RECORD</button>
    {/if}
    <span class="bpm">&hearts; <b>{hrBpm ?? '--'}</b> bpm</span>
    <div class="speed">
      {#each SPEEDS as s (s)}
        <button class="sp" class:on={speed === s} onclick={() => (speed = s)}>{s}</button>
      {/each}
      <span class="unit">mm/s</span>
    </div>
    <div class="stats">
      {#if status}
        <span class="state" class:err={status.state === 'error'} class:warn={busyOther}>
          {#if busyOther}
            strap busy with another capture - press RECORD for raw ECG
          {:else}
            {status.state}{status.detail ? ' - ' + status.detail : ''}
          {/if}
        </span>
      {/if}
      <span title="Total ECG samples received this session">{ecgTotal.toLocaleString()} ECG samples</span>
      <span title="Seconds of ECG recorded">{(ecgTotal / ECG_FS).toFixed(1)} s rec</span>
      <span class:warn={gaps > 0} title="Dropped ECG packets (missed BLE frames). Poor skin contact shows as NO SIGNAL on the strip, not here.">{gaps} dropped</span>
      <span>{accTotal.toLocaleString()} ACC</span>
      <span>device clock {deviceTimeS.toFixed(3)} s</span>
    </div>
  </div>
  <div class="scope ecgscope">
    <canvas bind:this={ecgCanvas}></canvas>
    <div class="scale">ECG - 130 Hz - {speed} mm/s - 10 mm/mV - red R-peak, amber ectopic</div>
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
  .ecg {
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
  button.rec {
    background: var(--color-accent);
  }
  button.rec:disabled {
    background: var(--color-disabled);
    cursor: not-allowed;
  }
  button.stop {
    background: var(--color-heart);
  }
  button.pause {
    background: var(--color-slate);
  }
  .bpm {
    font-size: 14px;
    color: var(--color-slate);
  }
  .bpm b {
    color: var(--color-heart);
    font-size: 18px;
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
  .stats .warn {
    color: var(--color-warning);
    font-weight: 600;
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
  .ecgscope {
    /* ~8 major (5 mm) rows = +/-2 mV, enough for the trace without wasting height */
    flex: 0 0 156px;
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

