<script lang="ts">
  import { onMount } from 'svelte'

  interface EcgMsg {
    source: string
    ts_device_ns: number
    ts_host_ns: number
    elapsed_ms: number
    samples: number[]
    total: number
    gaps: number
  }

  interface Props {
    sources: Record<string, string>
    send: (cmd: object) => void
    register: (fn: (m: EcgMsg) => void) => void
    onstatus: (source: string) => { state: string; detail: string; device: string } | null
  }
  let { sources, send, register, onstatus }: Props = $props()

  const SAMPLE_RATE = 130
  const WINDOW_S = 6

  let selected = $state('')
  let recording = $state(false)
  let total = $state(0)
  let gaps = $state(0)
  let deviceTimeS = $state(0)
  let canvas: HTMLCanvasElement | undefined = $state()

  // Ring buffer of recent samples for the scrolling waveform.
  const capacity = SAMPLE_RATE * WINDOW_S
  let ring = new Float32Array(capacity)
  let head = 0
  let filled = 0

  const sourceIds = $derived(Object.keys(sources))
  const status = $derived(selected ? onstatus(selected) : null)

  // Friendly names matching the HR-compare lanes, so the radio is obvious.
  function friendlyLabel(id: string): string {
    if (id.startsWith('raven:hci0')) return 'Raven - onboard AX211 (weak)'
    if (id.startsWith('raven:')) return 'Raven - ASUS BT-600 USB'
    return `Lenovo - native agent (${id.split(':')[0]})`
  }
  // Prefer any radio over the known-weak onboard AX211.
  function preferredSource(ids: string[]): string {
    return ids.find((id) => !id.startsWith('raven:hci0')) ?? ids[0] ?? ''
  }

  $effect(() => {
    if (!selected && sourceIds.length) selected = preferredSource(sourceIds)
  })

  onMount(() => {
    register((m: EcgMsg) => {
      if (m.source !== selected) return
      for (const s of m.samples) {
        ring[head] = s
        head = (head + 1) % capacity
        if (filled < capacity) filled++
      }
      total = m.total
      gaps = m.gaps
      deviceTimeS = m.ts_device_ns / 1e9
      draw()
    })
    const ro = new ResizeObserver(draw)
    if (canvas) ro.observe(canvas)
    let raf = requestAnimationFrame(function loop() {
      draw()
      raf = requestAnimationFrame(loop)
    })
    return () => {
      ro.disconnect()
      cancelAnimationFrame(raf)
    }
  })

  function resetBuffer() {
    ring = new Float32Array(capacity)
    head = 0
    filled = 0
    total = 0
    gaps = 0
  }

  function start() {
    if (!selected) return
    resetBuffer()
    recording = true
    send({ t: 'start', source: selected, mode: 'ecg', duration_s: 0 })
  }

  function stop() {
    recording = false
    if (selected) send({ t: 'stop', source: selected })
  }

  // React to stop/error status coming back from the agent.
  $effect(() => {
    const st = status?.state
    if (st === 'stopped' || st === 'error') recording = false
  })

  function draw() {
    if (!canvas) return
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

    // Ordered view of the ring buffer, oldest to newest.
    const n = filled
    const ordered = new Float32Array(n)
    for (let i = 0; i < n; i++) {
      ordered[i] = ring[(head - filled + i + capacity * 2) % capacity]
    }

    let min = Infinity
    let max = -Infinity
    for (let i = 0; i < n; i++) {
      if (ordered[i] < min) min = ordered[i]
      if (ordered[i] > max) max = ordered[i]
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

    if (n < 2) return
    ctx.strokeStyle = '#CB333B'
    ctx.lineWidth = 1.3
    ctx.lineJoin = 'round'
    ctx.beginPath()
    for (let i = 0; i < n; i++) {
      const x = (i / (capacity - 1)) * w
      const y = h - ((ordered[i] - min) / (max - min)) * h
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.stroke()
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
      <button class="rec" disabled={!selected} onclick={start}>RECORD ECG</button>
    {/if}
    <div class="stats">
      {#if status}
        <span class="state" class:err={status.state === 'error'}>
          {status.state}{status.detail ? ' - ' + status.detail : ''}
        </span>
      {/if}
      <span>{total.toLocaleString()} samples</span>
      <span>{(total / SAMPLE_RATE).toFixed(1)} s</span>
      <span class:warn={gaps > 0}>{gaps} gaps</span>
      <span>device clock {deviceTimeS.toFixed(3)} s</span>
    </div>
  </div>
  <div class="scope">
    <canvas bind:this={canvas}></canvas>
    <div class="scale">130 Hz - {WINDOW_S}s window - microvolts</div>
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
    flex: 1;
    position: relative;
    background: var(--color-card);
    border: 1px solid var(--color-line);
    border-radius: var(--radius);
    overflow: hidden;
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

