<script lang="ts">
  import { onMount } from 'svelte'
  import Lane from './lib/Lane.svelte'
  import { WebBTCapture } from './lib/webbt'
  import { computeMetrics, emptyLane } from './lib/metrics'
  import type { LaneData, Sample } from './lib/types'

  const LANE_DEFS = [
    { key: 'webbt', num: 1, title: 'LENOVO - WEB BLUETOOTH' },
    { key: 'lenovo', num: 2, title: 'LENOVO - NATIVE AGENT' },
    { key: 'ravenInt', num: 3, title: 'RAVEN - ONBOARD AX211' },
    { key: 'ravenUsb', num: 4, title: 'RAVEN - ASUS BT-600 USB' },
  ]

  let sources: Record<string, string> = $state({})
  let lanes: Record<string, LaneData> = $state({
    webbt: emptyLane(),
    lenovo: emptyLane(),
    ravenInt: emptyLane(),
    ravenUsb: emptyLane(),
  })
  let activeLane: string | null = $state(null)
  let durationS = $state(60)
  let wsUp = $state(false)

  let ws: WebSocket | null = null
  let closed = false
  const webbt = new WebBTCapture()
  let webbtTimer: ReturnType<typeof setTimeout> | null = null

  const RECORDING_STATES = ['scanning', 'connecting', 'streaming']

  function laneSource(key: string): string | null {
    const ids = Object.keys(sources)
    switch (key) {
      case 'webbt':
        return 'webbt'
      case 'lenovo':
        // Any non-raven agent (e.g. the laptop, whatever its hostname is).
        return ids.find((s) => !s.startsWith('raven:')) ?? null
      case 'ravenInt':
        return ids.includes('raven:hci0') ? 'raven:hci0' : null
      case 'ravenUsb':
        return ids.find((s) => s.startsWith('raven:') && s !== 'raven:hci0') ?? null
    }
    return null
  }

  function sourceLane(source: string): string | null {
    for (const d of LANE_DEFS) {
      if (laneSource(d.key) === source) return d.key
    }
    return null
  }

  onMount(() => {
    connectWs()
    return () => {
      closed = true
      ws?.close()
    }
  })

  function connectWs() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const s = new WebSocket(proto + '://' + location.host + '/ws/ui')
    s.onopen = () => {
      wsUp = true
    }
    s.onclose = () => {
      wsUp = false
      if (!closed) setTimeout(connectWs, 3000)
    }
    s.onmessage = (ev) => {
      try {
        handleMsg(JSON.parse(ev.data))
      } catch {
        // ignore malformed messages
      }
    }
    ws = s
  }

  function handleMsg(m: any) {
    if (m.t === 'sources') {
      const next: Record<string, string> = {}
      for (const s of m.sources) next[s.id] = s.label
      sources = next
    } else if (m.t === 'status') {
      const key = sourceLane(m.source)
      if (key) applyStatus(key, m)
    } else if (m.t === 'hr') {
      const key = sourceLane(m.source)
      if (key) {
        lanes[key].samples.push({ t: m.ts, bpm: m.bpm, rr: m.rr ?? [] })
      }
    }
  }

  function applyStatus(key: string, m: any) {
    const lane = lanes[key]
    if (m.battery != null) lane.battery = m.battery
    if (m.device) lane.device = m.device
    if (m.state === 'stopped') {
      if (RECORDING_STATES.includes(lane.status)) {
        lane.detail = m.detail ?? ''
        finishLane(key)
      }
    } else if (m.state === 'error') {
      let detail = m.detail ?? ''
      // Lane 3 (onboard AX211) is known-weak: point the user at the USB radio.
      if (
        key === 'ravenInt' &&
        (detail.includes('too weak') || detail.includes('no heart rate device'))
      ) {
        detail += ' - lane 3 onboard radio is too weak for recording, use USB lane 4 (ASUS BT-600)'
      }
      lane.detail = detail
      lane.status = 'error'
      if (activeLane === key) activeLane = null
    } else if (RECORDING_STATES.includes(m.state)) {
      lane.detail = m.detail ?? ''
      lane.status = m.state
    }
  }

  function finishLane(key: string) {
    const lane = lanes[key]
    lane.metrics = computeMetrics(lane.samples)
    lane.status = lane.samples.length ? 'done' : 'idle'
    if (activeLane === key) activeLane = null
    if (key === 'webbt' && webbtTimer) {
      clearTimeout(webbtTimer)
      webbtTimer = null
    }
  }

  function record(key: string) {
    if (activeLane) return
    const src = laneSource(key)
    if (!src) return
    const lane = lanes[key]
    lane.samples = []
    lane.metrics = null
    lane.battery = null
    lane.device = ''
    lane.detail = ''
    lane.status = 'scanning'
    activeLane = key

    if (key === 'webbt') {
      webbt.start({
        onstatus: (state, detail, extra) =>
          applyStatus('webbt', { state, detail, ...extra }),
        onsample: (s: Sample) => {
          lanes.webbt.samples.push(s)
        },
      })
      if (durationS > 0) {
        webbtTimer = setTimeout(() => webbt.stop('duration'), durationS * 1000)
      }
    } else {
      ws?.send(JSON.stringify({ t: 'start', source: src, duration_s: durationS }))
    }
  }

  function stop(key: string) {
    if (key === 'webbt') {
      webbt.stop('user')
    } else {
      const src = laneSource(key)
      if (src) ws?.send(JSON.stringify({ t: 'stop', source: src }))
    }
  }
</script>

<header>
  <div class="brand">ELDURO <span class="heart">&hearts;</span></div>
  <div class="subtitle">POLAR H10 SIGNAL LAB</div>
  <div class="controls">
    <label>
      Window
      <select bind:value={durationS}>
        <option value={30}>30 s</option>
        <option value={60}>60 s</option>
        <option value={120}>120 s</option>
        <option value={0}>manual</option>
      </select>
    </label>
    <span class="conn" class:up={wsUp}>
      {wsUp ? 'backend connected' : 'backend offline'}
    </span>
  </div>
</header>

<main>
  {#each LANE_DEFS as d (d.key)}
    <Lane
      num={d.num}
      title={d.title}
      lane={lanes[d.key]}
      available={d.key === 'webbt' || laneSource(d.key) !== null}
      locked={activeLane !== null && activeLane !== d.key}
      onrecord={() => record(d.key)}
      onstop={() => stop(d.key)}
    />
  {/each}
</main>

<style>
  header {
    height: 58px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 18px;
    border-bottom: 1px solid var(--color-line);
    background: var(--color-card);
  }
  .brand {
    font-family: var(--font-display);
    font-size: 30px;
    letter-spacing: 2px;
  }
  .brand .heart {
    color: var(--color-heart);
  }
  .subtitle {
    font-family: var(--font-display);
    font-size: 15px;
    letter-spacing: 2px;
    color: var(--color-slate);
    flex: 1;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 13px;
  }
  .controls select {
    margin-left: 6px;
    font-family: var(--font-body);
    padding: 3px 6px;
    border: 1px solid var(--color-line);
    border-radius: 6px;
    background: var(--color-bg);
  }
  .conn {
    color: var(--color-error);
    font-weight: 600;
  }
  .conn.up {
    color: var(--color-slate);
    font-weight: 400;
  }
  .conn.up::before,
  .conn::before {
    content: '';
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: 6px;
    background: currentColor;
  }
  main {
    height: calc(100vh - 58px);
    display: grid;
    grid-template-rows: repeat(4, 1fr);
    gap: 10px;
    padding: 10px 14px 14px;
    box-sizing: border-box;
  }
</style>

