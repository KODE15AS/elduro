<script lang="ts">
  import { onMount } from 'svelte'
  import type { EcgStreamMsg } from './types'

  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: EcgStreamMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, send, register, onstatus }: Props = $props()

  const ECG_RATE = 130
  const ACC_RATE = 200
  const WINDOW_S = 6

  let selected = $state('')
  let recording = $state(false)
  let ecgTotal = $state(0)
  let accTotal = $state(0)
  let gaps = $state(0)
  let deviceTimeS = $state(0)
  let ecgCanvas: HTMLCanvasElement | undefined = $state()
  let accCanvas: HTMLCanvasElement | undefined = $state()

  // Ring buffers for the scrolling scopes.
  const ecgCap = ECG_RATE * WINDOW_S
  const accCap = ACC_RATE * WINDOW_S
  let ecg = new Float32Array(ecgCap)
  let ecgHead = 0
  let ecgFilled = 0
  let accX = new Float32Array(accCap)
  let accY = new Float32Array(accCap)
  let accZ = new Float32Array(accCap)
  let accHead = 0
  let accFilled = 0

  const sourceIds = $derived(Object.keys(sources))
  const status = $derived(selected ? onstatus(selected) : null)

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
        for (const s of m.samples as number[]) {
          ecg[ecgHead] = s
          ecgHead = (ecgHead + 1) % ecgCap
          if (ecgFilled < ecgCap) ecgFilled++
        }
        ecgTotal = m.total
        gaps = m.gaps ?? 0
        deviceTimeS = m.ts_device_ns / 1e9
      } else if (m.t === 'acc') {
        for (const s of m.samples as number[][]) {
          accX[accHead] = s[0]
          accY[accHead] = s[1]
          accZ[accHead] = s[2]
          accHead = (accHead + 1) % accCap
          if (accFilled < accCap) accFilled++
        }
        accTotal = m.total
      }
    })
    let raf = requestAnimationFrame(function loop() {
      drawEcg()
      drawAcc()
      raf = requestAnimationFrame(loop)
    })
    return () => cancelAnimationFrame(raf)
  })

  function resetBuffers() {
    ecg = new Float32Array(ecgCap)
    ecgHead = 0
    ecgFilled = 0
    accX = new Float32Array(accCap)
    accY = new Float32Array(accCap)
    accZ = new Float32Array(accCap)
    accHead = 0
    accFilled = 0
    ecgTotal = 0
    accTotal = 0
    gaps = 0
  }

  function start() {
    if (!selected) return
    resetBuffers()
    recording = true
    send({ t: 'start', source: selected, mode: 'ecg', duration_s: 0 })
  }

  function stop() {
    recording = false
    if (selected) send({ t: 'stop', source: selected })
  }

  $effect(() => {
    const st = status?.state
    if (st === 'stopped' || st === 'error') recording = false
  })

  function prepare(canvas: HTMLCanvasElement): CanvasRenderingContext2D | null {
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
    return ctx
  }

  function ordered(ring: Float32Array, head: number, filled: number, cap: number): Float32Array {
    const out = new Float32Array(filled)
    for (let i = 0; i < filled; i++) {
      out[i] = ring[(head - filled + i + cap * 2) % cap]
    }
    return out
  }

  function drawLine(
    ctx: CanvasRenderingContext2D,
    data: Float32Array,
    cap: number,
    w: number,
    h: number,
    min: number,
    max: number,
    color: string,
  ) {
    if (data.length < 2 || max <= min) return
    ctx.strokeStyle = color
    ctx.lineWidth = 1.3
    ctx.lineJoin = 'round'
    ctx.beginPath()
    for (let i = 0; i < data.length; i++) {
      const x = (i / (cap - 1)) * w
      const y = h - ((data[i] - min) / (max - min)) * h
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.stroke()
  }

  function drawEcg() {
    if (!ecgCanvas) return
    const ctx = prepare(ecgCanvas)
    if (!ctx) return
    const w = ecgCanvas.clientWidth
    const h = ecgCanvas.clientHeight
    const data = ordered(ecg, ecgHead, ecgFilled, ecgCap)
    let min = Infinity
    let max = -Infinity
    for (const v of data) {
      if (v < min) min = v
      if (v > max) max = v
    }
    if (!isFinite(min) || !isFinite(max)) {
      min = -1000
      max = 1000
    }
    const pad = Math.max(50, (max - min) * 0.1)
    min -= pad
    max += pad
    ctx.strokeStyle = '#e3e0d8'
    ctx.lineWidth = 1
    ctx.beginPath()
    const zeroY = h - ((0 - min) / (max - min)) * h
    ctx.moveTo(0, zeroY)
    ctx.lineTo(w, zeroY)
    ctx.stroke()
    drawLine(ctx, data, ecgCap, w, h, min, max, '#CB333B')
  }

  function drawAcc() {
    if (!accCanvas) return
    const ctx = prepare(accCanvas)
    if (!ctx) return
    const w = accCanvas.clientWidth
    const h = accCanvas.clientHeight
    const xs = ordered(accX, accHead, accFilled, accCap)
    const ys = ordered(accY, accHead, accFilled, accCap)
    const zs = ordered(accZ, accHead, accFilled, accCap)
    let min = Infinity
    let max = -Infinity
    for (const arr of [xs, ys, zs]) {
      for (const v of arr) {
        if (v < min) min = v
        if (v > max) max = v
      }
    }
    if (!isFinite(min) || !isFinite(max)) {
      min = -2000
      max = 2000
    }
    const pad = Math.max(100, (max - min) * 0.1)
    min -= pad
    max += pad
    drawLine(ctx, xs, accCap, w, h, min, max, '#CB333B')
    drawLine(ctx, ys, accCap, w, h, min, max, '#40A15D')
    drawLine(ctx, zs, accCap, w, h, min, max, '#778395')
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
    {:else}
      <button class="rec" disabled={!selected} onclick={start}>RECORD</button>
    {/if}
    <div class="stats">
      {#if status}
        <span class="state" class:err={status.state === 'error'}>
          {status.state}{status.detail ? ' - ' + status.detail : ''}
        </span>
      {/if}
      <span>{ecgTotal.toLocaleString()} ECG</span>
      <span>{(ecgTotal / ECG_RATE).toFixed(1)} s</span>
      <span class:warn={gaps > 0}>{gaps} gaps</span>
      <span>{accTotal.toLocaleString()} ACC</span>
      <span>device clock {deviceTimeS.toFixed(3)} s</span>
    </div>
  </div>
  <div class="scope ecgscope">
    <canvas bind:this={ecgCanvas}></canvas>
    <div class="scale">ECG - 130 Hz - microvolts</div>
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
  .stats {
    display: flex;
    gap: 16px;
    font-size: 12px;
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
    flex: 2;
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
    font-size: 11px;
    color: var(--color-slate);
    background: rgba(243, 241, 236, 0.7);
    padding: 2px 8px;
    border-radius: 6px;
  }
</style>

